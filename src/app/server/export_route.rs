use crate::{
    app::server::app_state::AppState,
    render::{
        CustomLayer, CustomLayerOrder, ImageFormat, RenderLayer, RenderRequest, RenderWorkerPool,
        bbox_size_in_pixels,
    },
};
use axum::{
    body::Body,
    extract::{Json, Query, State},
    http::{Response, StatusCode},
};
use geo::Rect;
use geojson::{Feature, GeoJson};
use rand::TryRng;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    fs,
    sync::{Mutex, Notify, Semaphore},
    time::sleep,
};
use tokio_util::io::ReaderStream;

pub(crate) struct ExportState {
    jobs: Mutex<HashMap<String, Arc<ExportJob>>>,
    semaphore: Arc<Semaphore>,
    max_pixels: u64,
    abandon_grace: Duration,
}

impl ExportState {
    pub(crate) fn new(max_parallel: usize, max_pixels: u64, abandon_grace: Duration) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_parallel.max(1))),
            max_pixels,
            abandon_grace,
        }
    }
}

struct ExportJob {
    file_path: PathBuf,
    filename: String,
    content_type: &'static str,
    status: Arc<Mutex<ExportStatus>>,
    notify: Arc<Notify>,
    poller_count: Arc<AtomicUsize>,
    poller_change: Arc<Notify>,
    handle: tokio::task::JoinHandle<()>,
}

enum ExportStatus {
    Pending,
    Done(Result<(), ExportError>),
}

#[derive(Clone, Debug)]
enum ExportError {
    Abandoned,
    Render,
}

impl ExportError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Abandoned => StatusCode::GONE,
            Self::Render => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Deserialize, Debug)]
pub(crate) struct ExportRequest {
    zoom: u8,
    bbox: [f64; 4],
    format: Option<String>,
    scale: Option<f64>,
    features: Option<ExportFeatures>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportFeatures {
    shading: Option<bool>,
    contours: Option<bool>,
    bicycle_trails: Option<bool>,
    horse_trails: Option<bool>,
    hiking_trails: Option<bool>,
    ski_trails: Option<bool>,
    feature_collection: Option<serde_json::Value>,
    feature_collection_order: Option<CustomLayerOrder>,
}

#[derive(Deserialize)]
pub(crate) struct TokenQuery {
    token: String,
}

pub(crate) async fn post(
    State(state): State<AppState>,
    Json(request): Json<ExportRequest>,
) -> Response<Body> {
    let (format, ext, content_type) = match parse_format(request.format.as_deref()) {
        Ok(value) => value,
        Err(response) => return *response,
    };

    let scale = request.scale.unwrap_or(1.0);

    if !(scale.is_finite() && scale > 0.0) {
        return bad_request();
    }

    let bbox = bbox4326_to_3857(request.bbox);

    let rect = Rect::new((bbox[0], bbox[1]), (bbox[2], bbox[3]));

    let max_pixels = state.export_state.max_pixels;

    let estimated = {
        let size = bbox_size_in_pixels(rect, request.zoom as f64);
        (size.width as u64) * (size.height as u64)
    };

    if estimated > max_pixels {
        return Response::builder()
            .status(StatusCode::PAYLOAD_TOO_LARGE)
            .header("Content-Type", "application/json")
            .body(Body::from(
                json!({
                    "error": "export_too_large",
                    "estimatedPixels": estimated,
                    "maxPixels": max_pixels,
                })
                .to_string(),
            ))
            .expect("too large body");
    }

    let token = generate_token();

    let filename = format!("export-{token}.{ext}");

    let file_path = std::env::temp_dir().join(&filename);

    let mut render = state.default_render.to_owned();

    if let Some(features) = &request.features {
        if let Some(shading) = features.shading {
            if shading {
                render.insert(RenderLayer::Shading);
            } else {
                render.remove(&RenderLayer::Shading);
            }
        }

        if let Some(contours) = features.contours {
            if contours {
                render.insert(RenderLayer::Contours);
            } else {
                render.remove(&RenderLayer::Contours);
            }
        }

        if let Some(value) = features.hiking_trails {
            if value {
                render.insert(RenderLayer::RoutesHiking);
            } else {
                render.remove(&RenderLayer::RoutesHiking);
            }
        }

        if let Some(value) = features.horse_trails {
            if value {
                render.insert(RenderLayer::RoutesHorse);
            } else {
                render.remove(&RenderLayer::RoutesHorse);
            }
        }

        if let Some(value) = features.bicycle_trails {
            if value {
                render.insert(RenderLayer::RoutesBicycle);
            } else {
                render.remove(&RenderLayer::RoutesBicycle);
            }
        }

        if let Some(value) = features.ski_trails {
            if value {
                render.insert(RenderLayer::RoutesSki);
            } else {
                render.remove(&RenderLayer::RoutesSki);
            }
        }
    }

    let mut render_request = RenderRequest::new(rect, request.zoom, scale, format, render, None);

    render_request.custom_layer = if let Some(export_features) = &request.features
        && let Some(feature_collection) = &export_features.feature_collection
    {
        match serde_json::from_value::<GeoJson>(feature_collection.clone())
            .map_err(|_err| "error parsing geojson")
            .and_then(geojson_to_features)
        {
            Ok(features) => Some(CustomLayer {
                features,
                order: export_features
                    .feature_collection_order
                    .unwrap_or(CustomLayerOrder::Topmost),
            }),
            Err(_) => return bad_request(),
        }
    } else {
        None
    };

    let job = spawn_export_job(
        state.render_worker_pool.clone(),
        state.export_state.semaphore.clone(),
        state.export_state.abandon_grace,
        file_path.clone(),
        filename.clone(),
        content_type,
        render_request,
    );

    state
        .export_state
        .jobs
        .lock()
        .await
        .insert(token.clone(), job);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "token": token }).to_string()))
        .expect("token body")
}

pub(crate) async fn head(
    State(state): State<AppState>,
    Query(query): Query<TokenQuery>,
) -> Response<Body> {
    let Some(job) = get_job(&state, &query.token).await else {
        return not_found();
    };

    let _poller = PollerGuard::new(job.poller_count.clone(), job.poller_change.clone());

    match wait_job(&job).await {
        Ok(()) => Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .expect("head body"),
        Err(err) => Response::builder()
            .status(err.status_code())
            .body(Body::empty())
            .expect("head error body"),
    }
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Query(query): Query<TokenQuery>,
) -> Response<Body> {
    let Some(job) = get_job(&state, &query.token).await else {
        return not_found();
    };

    let _poller = PollerGuard::new(job.poller_count.clone(), job.poller_change.clone());

    if let Err(err) = wait_job(&job).await {
        return Response::builder()
            .status(err.status_code())
            .body(Body::empty())
            .expect("get error body");
    }

    let file = match fs::File::open(&job.file_path).await {
        Ok(file) => file,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .expect("read error body");
        }
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", job.content_type)
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", job.filename),
        )
        .body(body)
        .expect("download body")
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    Query(query): Query<TokenQuery>,
) -> Response<Body> {
    let job = {
        let mut jobs = state.export_state.jobs.lock().await;

        jobs.remove(&query.token)
    };

    let Some(job) = job else {
        return not_found();
    };

    job.handle.abort();

    let _ = fs::remove_file(&job.file_path).await;

    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .expect("delete body")
}

fn generate_token() -> String {
    let mut bytes = [0_u8; 16];

    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("os rng error");

    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_format(
    format: Option<&str>,
) -> Result<(ImageFormat, &'static str, &'static str), Box<Response<Body>>> {
    let format = format.unwrap_or("pdf");

    match format {
        "pdf" => Ok((ImageFormat::Pdf, "pdf", "application/pdf")),
        "svg" => Ok((ImageFormat::Svg, "svg", "image/svg+xml")),
        "jpeg" => Ok((ImageFormat::Jpeg, "jpeg", "image/jpeg")),
        "jpg" => Ok((ImageFormat::Jpeg, "jpg", "image/jpeg")),
        "png" => Ok((ImageFormat::Png, "png", "image/png")),
        _ => Err(Box::new(bad_request())),
    }
}

fn geojson_to_features(geojson: GeoJson) -> Result<Vec<Feature>, &'static str> {
    match geojson {
        GeoJson::FeatureCollection(collection) => Ok(collection.features),
        GeoJson::Feature(feature) => Ok(vec![feature]),
        _ => Err("unsupported geojson"),
    }
}

fn bbox4326_to_3857(bbox: [f64; 4]) -> [f64; 4] {
    let (min_x, min_y) = lon_lat_to_3857(bbox[0], bbox[1]);
    let (max_x, max_y) = lon_lat_to_3857(bbox[2], bbox[3]);
    [min_x, min_y, max_x, max_y]
}

fn lon_lat_to_3857(lon: f64, lat: f64) -> (f64, f64) {
    const EARTH_RADIUS: f64 = 6_378_137.0;
    const MAX_LAT: f64 = 85.05112878;

    let clamped_lat = lat.clamp(-MAX_LAT, MAX_LAT);
    let x = (lon.to_radians()) * EARTH_RADIUS;
    let y = (clamped_lat.to_radians() / 2.0 + std::f64::consts::FRAC_PI_4)
        .tan()
        .ln()
        * EARTH_RADIUS;

    (x, y)
}

struct PollerGuard {
    count: Arc<AtomicUsize>,
    notify: Arc<Notify>,
}

impl PollerGuard {
    fn new(count: Arc<AtomicUsize>, notify: Arc<Notify>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        notify.notify_waiters();
        Self { count, notify }
    }
}

impl Drop for PollerGuard {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

fn spawn_export_job(
    worker_pool: Arc<RenderWorkerPool>,
    semaphore: Arc<Semaphore>,
    abandon_grace: Duration,
    file_path: PathBuf,
    filename: String,
    content_type: &'static str,
    request: RenderRequest,
) -> Arc<ExportJob> {
    let status = Arc::new(Mutex::new(ExportStatus::Pending));
    let notify = Arc::new(Notify::new());
    let poller_count = Arc::new(AtomicUsize::new(0));
    let poller_change = Arc::new(Notify::new());

    let status_clone = Arc::clone(&status);
    let notify_clone = Arc::clone(&notify);
    let poller_count_clone = Arc::clone(&poller_count);
    let poller_change_clone = Arc::clone(&poller_change);
    let file_path_clone = file_path.clone();

    let handle = tokio::spawn(async move {
        let permit = match wait_for_permit(
            semaphore,
            poller_count_clone,
            poller_change_clone,
            abandon_grace,
        )
        .await
        {
            Some(permit) => permit,
            None => {
                let mut guard = status_clone.lock().await;
                *guard = ExportStatus::Done(Err(ExportError::Abandoned));
                notify_clone.notify_waiters();
                return;
            }
        };

        let result = run_export(worker_pool, file_path_clone, request)
            .await
            .map_err(|err| {
                eprintln!("export render failed: {err}");
                ExportError::Render
            });

        drop(permit);

        let mut guard = status_clone.lock().await;
        *guard = ExportStatus::Done(result);
        notify_clone.notify_waiters();
    });

    Arc::new(ExportJob {
        file_path,
        filename,
        content_type,
        status,
        notify,
        poller_count,
        poller_change,
        handle,
    })
}

async fn wait_for_permit(
    semaphore: Arc<Semaphore>,
    poller_count: Arc<AtomicUsize>,
    poller_change: Arc<Notify>,
    abandon_grace: Duration,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    // Abandon the job once no client has been actively polling for
    // `abandon_grace`. The grace also covers the gap between POST and
    // the client's first poll. The `acquire_owned` future is kept alive
    // throughout so the job keeps its FIFO position in the wait queue.
    let watchdog = async {
        loop {
            // Subscribe before reading state to avoid missing a change
            // notification that arrives between the check and the await.
            let changed = poller_change.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();

            if poller_count.load(Ordering::SeqCst) > 0 {
                changed.await;
                continue;
            }

            tokio::select! {
                _ = sleep(abandon_grace) => return,
                _ = &mut changed => continue,
            }
        }
    };

    tokio::select! {
        res = semaphore.acquire_owned() => res.ok(),
        _ = watchdog => None,
    }
}

async fn run_export(
    worker_pool: Arc<RenderWorkerPool>,
    file_path: PathBuf,
    request: RenderRequest,
) -> Result<(), String> {
    let image = worker_pool
        .render(request)
        .await
        .map_err(|err| err.to_string())?;

    fs::write(&file_path, image)
        .await
        .map_err(|err| err.to_string())?;

    Ok(())
}

async fn get_job(state: &AppState, token: &str) -> Option<Arc<ExportJob>> {
    let jobs = state.export_state.jobs.lock().await;
    jobs.get(token).cloned()
}

async fn wait_job(job: &ExportJob) -> Result<(), ExportError> {
    loop {
        let notified = {
            let guard = job.status.lock().await;

            match &*guard {
                ExportStatus::Pending => job.notify.notified(),
                ExportStatus::Done(result) => return result.clone(),
            }
        };

        notified.await;
    }
}

fn bad_request() -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .expect("bad request body")
}

fn not_found() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::empty())
        .expect("not found body")
}
