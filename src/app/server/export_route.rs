use crate::{
    app::server::{app_state::AppState, routes::ServerOptions},
    render::{
        ATTRIBUTION_HEADER, Attribution, AttributionDecoration, CustomLayer, CustomLayerOrder,
        Decorations, Glow, ImageFormat, LabelStyle, Layers, RenderLayer, RenderRequest,
        RenderWorkerPool,
        bbox_size_in_pixels,
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
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    fs,
    sync::{Mutex, Notify, Semaphore, watch},
    time::sleep,
};
use tokio_util::io::ReaderStream;

pub struct ExportState {
    jobs: Mutex<HashMap<String, Arc<ExportJob>>>,
    semaphore: Arc<Semaphore>,
    max_pixels: u64,
    abandon_grace: Duration,
    retention: Duration,
}

impl ExportState {
    pub(crate) fn new(options: &ServerOptions) -> Self {
        Self {
            jobs: Mutex::default(),
            semaphore: Arc::new(Semaphore::new(options.max_parallel_exports.max(1))),
            max_pixels: options.max_export_pixels,
            abandon_grace: options.export_abandon_grace,
            retention: options.export_retention,
        }
    }
}

/// Where the finished render lands and how it is served back.
struct ExportOutput {
    file_path: PathBuf,
    filename: String,
    content_type: &'static str,
}

struct ExportJob {
    token: String,
    output: ExportOutput,
    status: Mutex<ExportStatus>,
    notify: Notify,
    poller_count: AtomicUsize,
    poller_change: Notify,
    cancel: watch::Sender<bool>,
}

/// The temp file belongs to the job, so it goes when the last holder — a
/// streaming download, or the job's own task — lets go. No code path has to
/// remember to unlink it.
impl Drop for ExportJob {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.output.file_path);
    }
}

enum ExportStatus {
    Pending,
    Done(Result<Attribution, ExportError>),
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
    attribution: Option<ExportAttribution>,
}

/// Asks for the attribution line. Its text is not the client's to send: only the
/// render knows which datasets it drew from, so the client contributes what the
/// renderer cannot know and the renderer appends the rest.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportAttribution {
    /// Whatever the exported features earn — routers above all. Drawn after this
    /// map's own credit, in the order given.
    #[serde(default)]
    extra: Vec<String>,
    /// Titles the client words itself, by dataset code, replacing `/licenses`'.
    /// Only OpenStreetMap's is translated; the rest are the rights-holders' own
    /// strings, the same in every language.
    #[serde(default)]
    titles: HashMap<String, String>,
}

impl ExportAttribution {
    /// `/export` takes no credentials, and every credit sent is measured and
    /// drawn, so a caller cannot hand over an unbounded list of them.
    fn within_limits(&self) -> bool {
        const MAX_ENTRIES: usize = 32;
        const MAX_LEN: usize = 200;

        self.extra.len() <= MAX_ENTRIES
            && self.titles.len() <= MAX_ENTRIES
            && self.extra.iter().all(|credit| credit.len() <= MAX_LEN)
            && self.titles.values().all(|title| title.len() <= MAX_LEN)
    }
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
    /// Draw an overlay — nothing but `layers`, over a transparent background —
    /// instead of the map. Needs an alpha-capable `format`, and the server
    /// defaults do not apply: what is not listed is not drawn.
    #[serde(default)]
    only: bool,
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

    let only = request.features.as_ref().is_some_and(|features| features.only);

    let mut render = if only {
        HashSet::new()
    } else {
        state.default_render.clone()
    };

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

    let layers = if only {
        Layers::Only(render)
    } else {
        Layers::Map(render)
    };

    let mut render_request = RenderRequest::new(rect, request.zoom, scale, format, layers, None);

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

    let credits_over_limit = request
        .decorations
        .as_ref()
        .and_then(|d| d.attribution.as_ref())
        .is_some_and(|a| !a.within_limits());

    if credits_over_limit {
        return bad_request();
    }

    render_request.decorations = request.decorations.as_ref().and_then(|d| {
        let trimmed = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };

        let scale_bar = d.scale_bar.unwrap_or(false);
        let north_arrow = trimmed(&d.north_arrow);

        let attribution = d.attribution.as_ref().map(|a| AttributionDecoration {
            extra: a
                .extra
                .iter()
                .map(|credit| single_line(credit))
                .filter(|credit| !credit.is_empty())
                .collect(),
            catalog: state.licenses.titles(),
            overrides: a
                .titles
                .iter()
                .map(|(code, title)| (code.trim().to_owned(), single_line(title)))
                .filter(|(_, title)| !title.is_empty())
                .collect(),
        });

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

    spawn_export_job(
        &state,
        token.clone(),
        ExportOutput {
            file_path,
            filename,
            content_type,
        },
        render_request,
    )
    .await;

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

    let _poller = PollerGuard::new(&job);

    match wait_job(&job).await {
        Ok(attribution) => Response::builder()
            .status(StatusCode::OK)
            .header(ATTRIBUTION_HEADER, attribution.encode())
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

    let _poller = PollerGuard::new(&job);

    let attribution = match wait_job(&job).await {
        Ok(attribution) => attribution,
        Err(err) => {
            return Response::builder()
                .status(err.status_code())
                .body(Body::empty())
                .expect("get error body");
        }
    };

    let file = match fs::File::open(&job.output.file_path).await {
        Ok(file) => file,
        // Retention can drop the file between the lookup and here; the token is
        // on its way out either way, so answer as if it had already expired.
        Err(err) if err.kind() == ErrorKind::NotFound => return not_found(),
        Err(err) => {
            eprintln!("export file unreadable: {err}");

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
        .header("Content-Type", job.output.content_type)
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", job.output.filename),
        )
        .header(ATTRIBUTION_HEADER, attribution.encode())
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

    // Cancelling lets the job wind itself down; the file goes with it. Aborting
    // the task here would race the render instead: the write is a blocking op
    // that no cancellation stops, so it would recreate a file removed here.
    let _ = job.cancel.send(true);

    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .expect("delete body")
}

/// A credit as one drawn line. `draw_text` honours an embedded newline while
/// the wrap measures only the widest part, so a caller could otherwise push the
/// block off the canvas. No-break spaces are left alone — the credits use them.
fn single_line(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split(' ')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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

struct PollerGuard<'a> {
    job: &'a ExportJob,
}

impl<'a> PollerGuard<'a> {
    fn new(job: &'a ExportJob) -> Self {
        job.poller_count.fetch_add(1, Ordering::SeqCst);
        job.poller_change.notify_waiters();
        Self { job }
    }
}

impl Drop for PollerGuard<'_> {
    fn drop(&mut self) {
        self.job.poller_count.fetch_sub(1, Ordering::SeqCst);
        self.job.poller_change.notify_waiters();
    }
}

/// Spawns the render and the job's own wind-down: a client that has taken its
/// file deletes the job, and `retention` is the backstop for one that never
/// comes back, without which the map would grow for the life of the process.
async fn spawn_export_job(
    state: &AppState,
    token: String,
    output: ExportOutput,
    request: RenderRequest,
) {
    let (cancel, mut cancel_rx) = watch::channel(false);

    let job = Arc::new(ExportJob {
        token,
        output,
        status: Mutex::new(ExportStatus::Pending),
        notify: Notify::new(),
        poller_count: AtomicUsize::new(0),
        poller_change: Notify::new(),
        cancel,
    });

    // Register before the task starts. The task drops the job by token, and a
    // short retention would otherwise let it run before this insert, stranding
    // an entry whose file is already gone.
    let key = job.token.clone();
    let entry = Arc::clone(&job);

    state.export_state.jobs.lock().await.insert(key, entry);

    let export_state = Arc::clone(&state.export_state);
    let worker_pool = Arc::clone(&state.render_worker_pool);

    tokio::spawn(async move {
        let result = match wait_for_permit(&job, &export_state, &mut cancel_rx).await {
            Some(_permit) => run_export(worker_pool, &job.output.file_path, request)
                .await
                .map_err(|err| {
                    eprintln!("export render failed: {err}");
                    ExportError::Render
                }),
            None => Err(ExportError::Abandoned),
        };

        let mut guard = job.status.lock().await;
        *guard = ExportStatus::Done(result);
        drop(guard);
        job.notify.notify_waiters();

        tokio::select! {
            () = sleep(export_state.retention) => {}
            () = cancelled(&mut cancel_rx) => {}
        }

        export_state.jobs.lock().await.remove(&job.token);
    });
}

/// Resolves once the job is cancelled. The task holds the job, and with it the
/// sender, so the channel cannot close while this is awaited.
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    let _ = cancel.wait_for(|cancelled| *cancelled).await;
}

async fn wait_for_permit(
    job: &ExportJob,
    export_state: &ExportState,
    cancel: &mut watch::Receiver<bool>,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    // Abandon the job once no client has been actively polling for
    // `abandon_grace`. The grace also covers the gap between POST and
    // the client's first poll. The `acquire_owned` future is kept alive
    // throughout so the job keeps its FIFO position in the wait queue.
    let watchdog = async {
        loop {
            // Subscribe before reading state to avoid missing a change
            // notification that arrives between the check and the await.
            let changed = job.poller_change.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();

            if job.poller_count.load(Ordering::SeqCst) > 0 {
                changed.await;
                continue;
            }

            tokio::select! {
                () = sleep(export_state.abandon_grace) => return,
                () = &mut changed => {}
            }
        }
    };

    tokio::select! {
        res = export_state.semaphore.clone().acquire_owned() => res.ok(),
        () = watchdog => None,
        () = cancelled(cancel) => None,
    }
}

async fn run_export(
    worker_pool: Arc<RenderWorkerPool>,
    file_path: &Path,
    request: RenderRequest,
) -> Result<Attribution, String> {
    let image = worker_pool
        .render(request)
        .await
        .map_err(|err| err.to_string())?;

    fs::write(&file_path, image.bytes)
        .await
        .map_err(|err| err.to_string())?;

    Ok(image.attribution)
}

async fn get_job(state: &AppState, token: &str) -> Option<Arc<ExportJob>> {
    let jobs = state.export_state.jobs.lock().await;
    jobs.get(token).cloned()
}

async fn wait_job(job: &ExportJob) -> Result<Attribution, ExportError> {
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
