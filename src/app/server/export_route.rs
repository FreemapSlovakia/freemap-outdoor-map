use crate::{
    app::server::app_state::AppState,
    render::{
        CustomLayer, CustomLayerOrder, Decorations, Glow, ImageFormat, LabelStyle, RenderLayer,
        RenderRequest, RenderWorkerPool, bbox_size_in_pixels,
    },
};
use axum::{
    body::Body,
    extract::{Json, Query, State},
    http::{Response, StatusCode},
};
use colorsys::{Rgb, RgbRatio};
use cosmic_text::Weight;
use geo::Rect;
use geojson::{Feature, GeoJson};
use rand::TryRng;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
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

pub struct ExportState {
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
    const fn status_code(&self) -> StatusCode {
        match self {
            Self::Abandoned => StatusCode::GONE,
            Self::Render => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct ExportRequest {
    zoom: u8,
    bbox: [f64; 4],
    format: Option<String>,
    scale: Option<f64>,
    features: Option<ExportFeatures>,
    decorations: Option<ExportDecorations>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportDecorations {
    scale_bar: Option<bool>,
    north_arrow: Option<String>,
    attribution: Option<String>,
}

/// Client-toggleable map layers. Each maps to one [`RenderLayer`]; the set sent
/// in the request lists exactly which of these are enabled (membership = on).
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ExportLayer {
    Shading,
    Contours,
    BicycleTrails,
    HorseTrails,
    HikingTrails,
    SkiTrails,
}

impl ExportLayer {
    const ALL: [Self; 6] = [
        Self::Shading,
        Self::Contours,
        Self::BicycleTrails,
        Self::HorseTrails,
        Self::HikingTrails,
        Self::SkiTrails,
    ];

    const fn render_layer(self) -> RenderLayer {
        match self {
            Self::Shading => RenderLayer::Shading,
            Self::Contours => RenderLayer::Contours,
            Self::BicycleTrails => RenderLayer::RoutesBicycle,
            Self::HorseTrails => RenderLayer::RoutesHorse,
            Self::HikingTrails => RenderLayer::RoutesHiking,
            Self::SkiTrails => RenderLayer::RoutesSki,
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportFeatures {
    /// Toggleable layers that are enabled. Absent keeps the server defaults; a
    /// present set explicitly turns each toggleable layer on (in set) or off.
    layers: Option<HashSet<ExportLayer>>,
    /// Custom `GeoJSON` overlay layer and its rendering options. Absent means no
    /// overlay.
    custom_layer: Option<ExportCustomLayer>,
}

/// A custom `GeoJSON` overlay plus its rendering options. The options
/// (`feature_collection_order`, `glow_color`, `glow_width`, `label_color`,
/// `label_weight`, `label_size`) only make sense when there are features to
/// draw, so they live here alongside the (mandatory) `feature_collection`
/// rather than on [`ExportFeatures`]. Marker size is baked into each feature's
/// `marker-svg` (drawn at its natural size), so there is no marker-width field.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportCustomLayer {
    /// `GeoJSON` `FeatureCollection` to render as the overlay.
    feature_collection: serde_json::Value,
    /// Where in the layer stack to draw the overlay. Defaults to
    /// [`CustomLayerOrder::Topmost`].
    feature_collection_order: Option<CustomLayerOrder>,
    /// Glow halo color for the custom features, as a CSS color string. The alpha
    /// channel is the glow opacity (e.g. `#00000040` or `rgba(0,0,0,0.25)`).
    /// Omitted/empty disables the glow.
    glow_color: Option<String>,
    /// Width (in tile/CSS pixels) the glow halo extends on each side. Defaults to
    /// [`DEFAULT_GLOW_WIDTH`]. Only used when `glow_color` is set.
    glow_width: Option<f64>,
    /// Text color for feature `title` labels, as a CSS color string (e.g.
    /// `#0000ff` or `rgb(0,0,255)`). Omitted/empty keeps the per-kind default
    /// (blue for point labels, black for line/polygon labels).
    label_color: Option<String>,
    /// Font weight for feature `title` labels (e.g. `400` normal, `700` bold).
    /// Omitted keeps the per-kind default (bold for point labels, normal for
    /// line/polygon labels).
    label_weight: Option<u16>,
    /// Font size (in tile/CSS pixels) for feature `title` labels. Omitted keeps
    /// the default ([`DEFAULT_LABEL_SIZE`]).
    label_size: Option<f64>,
}

/// Default per-side glow halo width.
const DEFAULT_GLOW_WIDTH: f64 = 2.0;

/// Default font size (tile/CSS px) for custom-feature labels, matching the
/// in-app overlay label size.
const DEFAULT_LABEL_SIZE: f64 = 15.0;

#[derive(Deserialize)]
pub struct TokenQuery {
    token: String,
}

pub async fn post(
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

    let mut render = state.default_render.clone();

    if let Some(features) = &request.features
        && let Some(layers) = &features.layers
    {
        for export_layer in ExportLayer::ALL {
            let render_layer = export_layer.render_layer();

            if layers.contains(&export_layer) {
                render.insert(render_layer);
            } else {
                render.remove(&render_layer);
            }
        }
    }

    let mut render_request = RenderRequest::new(rect, request.zoom, scale, format, render, None);

    render_request.custom_layer = if let Some(custom_layer) = request
        .features
        .as_ref()
        .and_then(|features| features.custom_layer.as_ref())
    {
        let glow_width = custom_layer.glow_width.unwrap_or(DEFAULT_GLOW_WIDTH);

        if !(glow_width.is_finite() && glow_width >= 0.0) {
            return bad_request();
        }

        if let Some(label_size) = custom_layer.label_size
            && !(label_size.is_finite() && label_size > 0.0)
        {
            return bad_request();
        }

        let glow_color = match custom_layer
            .glow_color
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => match parse_glow(s) {
                Some(color) => Some(Glow {
                    color,
                    width: glow_width,
                }),
                None => return bad_request(),
            },
            None => None,
        };

        let label_color = match custom_layer
            .label_color
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => match parse_glow(s) {
                Some(color) => Some((color.r(), color.g(), color.b())),
                None => return bad_request(),
            },
            None => None,
        };

        let label_style = LabelStyle {
            color: label_color,
            weight: custom_layer.label_weight.map(Weight),
            size: custom_layer.label_size.or(Some(DEFAULT_LABEL_SIZE)),
        };

        match serde_json::from_value::<GeoJson>(custom_layer.feature_collection.clone())
            .map_err(|_err| "error parsing geojson")
            .and_then(geojson_to_features)
        {
            Ok(features) => Some(CustomLayer {
                features,
                order: custom_layer
                    .feature_collection_order
                    .unwrap_or(CustomLayerOrder::Topmost),
                glow_color,
                label_style,
            }),
            Err(_) => return bad_request(),
        }
    } else {
        None
    };

    render_request.decorations = request.decorations.as_ref().and_then(|d| {
        let trimmed = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };

        let scale_bar = d.scale_bar.unwrap_or(false);
        let north_arrow = trimmed(&d.north_arrow);
        let attribution = trimmed(&d.attribution);

        if !scale_bar && north_arrow.is_none() && attribution.is_none() {
            return None;
        }

        Some(Decorations {
            scale_bar,
            north_arrow,
            attribution,
            // Center latitude of the original WGS84 bbox, used to correct the
            // Web-Mercator scale for the scale bar.
            center_lat: f64::midpoint(request.bbox[1], request.bbox[3]),
        })
    });

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

pub async fn head(
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

pub async fn get(State(state): State<AppState>, Query(query): Query<TokenQuery>) -> Response<Body> {
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

    let Ok(file) = fs::File::open(&job.file_path).await else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .expect("read error body");
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

pub async fn delete(
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

    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
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

/// Parse a CSS color string (hex `#rgb`/`#rrggbb`/`#rrggbbaa` or
/// `rgb()`/`rgba()`) into an `RgbRatio`, whose alpha carries the glow opacity.
fn parse_glow(s: &str) -> Option<RgbRatio> {
    Rgb::from_hex_str(s)
        .or_else(|_| s.parse::<Rgb>())
        .ok()
        .map(|rgb| rgb.as_ratio())
}

fn geojson_to_features(geojson: GeoJson) -> Result<Vec<Feature>, &'static str> {
    match geojson {
        GeoJson::FeatureCollection(collection) => Ok(collection.features),
        GeoJson::Feature(feature) => Ok(vec![feature]),
        GeoJson::Geometry(_) => Err("unsupported geojson"),
    }
}

fn bbox4326_to_3857(bbox: [f64; 4]) -> [f64; 4] {
    let (min_x, min_y) = lon_lat_to_3857(bbox[0], bbox[1]);
    let (max_x, max_y) = lon_lat_to_3857(bbox[2], bbox[3]);
    [min_x, min_y, max_x, max_y]
}

fn lon_lat_to_3857(lon: f64, lat: f64) -> (f64, f64) {
    const EARTH_RADIUS: f64 = 6_378_137.0;
    const MAX_LAT: f64 = 85.051_128_78;

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
        let Some(permit) = wait_for_permit(
            semaphore,
            poller_count_clone,
            poller_change_clone,
            abandon_grace,
        )
        .await
        else {
            let mut guard = status_clone.lock().await;
            *guard = ExportStatus::Done(Err(ExportError::Abandoned));
            drop(guard);
            notify_clone.notify_waiters();
            return;
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
        drop(guard);
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
                () = sleep(abandon_grace) => return,
                () = &mut changed => {}
            }
        }
    };

    tokio::select! {
        res = semaphore.acquire_owned() => res.ok(),
        () = watchdog => None,
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
