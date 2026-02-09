use crate::{
    app::server::app_state::AppState,
    render::{ImageFormat, RenderRequest, RenderWorkerPool, RouteTypes},
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
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    fs,
    sync::{Mutex, Notify},
};
use tokio_util::io::ReaderStream;

#[derive(Default)]
pub(crate) struct ExportState {
    jobs: Mutex<HashMap<String, Arc<ExportJob>>>,
}

impl ExportState {
    pub(crate) fn new() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
        }
    }
}

struct ExportJob {
    file_path: PathBuf,
    filename: String,
    content_type: &'static str,
    status: Arc<Mutex<ExportStatus>>,
    notify: Arc<Notify>,
    handle: tokio::task::JoinHandle<()>,
}

enum ExportStatus {
    Pending,
    Done(Result<(), String>),
}

#[derive(Deserialize)]
pub(crate) struct ExportRequest {
    zoom: u8,
    bbox: [f64; 4],
    format: Option<String>,
    scale: Option<f64>,
    features: Option<ExportFeatures>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportFeatures {
    shading: Option<bool>,
    contours: Option<bool>,
    bicycle_trails: Option<bool>,
    horse_trails: Option<bool>,
    hiking_trails: Option<bool>,
    ski_trails: Option<bool>,
    feature_collection: Option<serde_json::Value>,
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

    let token = generate_token();

    let filename = format!("export-{token}.{ext}");

    let file_path = std::env::temp_dir().join(&filename);

    let render_request = match build_render_request(&request, format, scale) {
        Ok(value) => value,
        Err(response) => return *response,
    };

    let job = spawn_export_job(
        state.render_worker_pool.clone(),
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

    match wait_job(&job).await {
        Ok(()) => Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .expect("head body"),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
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

    if (wait_job(&job).await).is_err() {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
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

fn build_render_request(
    request: &ExportRequest,
    format: ImageFormat,
    scale: f64,
) -> Result<RenderRequest, Box<Response<Body>>> {
    let bbox = bbox4326_to_3857(request.bbox);

    let rect = Rect::new((bbox[0], bbox[1]), (bbox[2], bbox[3]));

    let mut render_request = RenderRequest::new(rect, request.zoom, scale, format);

    if let Some(features) = &request.features {
        if let Some(shading) = features.shading {
            render_request.shading = shading;
        }

        if let Some(contours) = features.contours {
            render_request.contours = contours;
        }

        if let Some(feature_collection) = &features.feature_collection {
            let geojson = serde_json::from_value::<GeoJson>(feature_collection.clone())
                .map_err(|_| Box::new(bad_request()))?;
            let features = geojson_to_features(geojson).map_err(|_| Box::new(bad_request()))?;
            render_request.featues = Some(features);
        }

        let mut any_route_flag = false;

        let mut routes = RouteTypes::empty();

        if let Some(value) = features.hiking_trails {
            any_route_flag = true;

            if value {
                routes |= RouteTypes::HIKING;
            }
        }

        if let Some(value) = features.horse_trails {
            any_route_flag = true;

            if value {
                routes |= RouteTypes::HORSE;
            }
        }

        if let Some(value) = features.bicycle_trails {
            any_route_flag = true;

            if value {
                routes |= RouteTypes::BICYCLE;
            }
        }

        if let Some(value) = features.ski_trails {
            any_route_flag = true;

            if value {
                routes |= RouteTypes::SKI;
            }
        }

        if any_route_flag {
            render_request.route_types = routes;
        }
    }

    Ok(render_request)
}

fn geojson_to_features(geojson: GeoJson) -> Result<Vec<Feature>, String> {
    match geojson {
        GeoJson::FeatureCollection(collection) => Ok(collection.features),
        GeoJson::Feature(feature) => Ok(vec![feature]),
        _ => Err("unsupported geojson".into()),
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

fn spawn_export_job(
    worker_pool: Arc<RenderWorkerPool>,
    file_path: PathBuf,
    filename: String,
    content_type: &'static str,
    request: RenderRequest,
) -> Arc<ExportJob> {
    let status = Arc::new(Mutex::new(ExportStatus::Pending));
    let notify = Arc::new(Notify::new());
    let status_clone = Arc::clone(&status);
    let notify_clone = Arc::clone(&notify);

    let file_path_clone = file_path.clone();
    let handle = tokio::spawn(async move {
        let result = run_export(worker_pool, file_path_clone, request).await;
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
        handle,
    })
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

async fn wait_job(job: &ExportJob) -> Result<(), String> {
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
