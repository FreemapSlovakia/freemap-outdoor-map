use crate::{
    app::{
        server::{
            app_state::{AppState, TileRouteState, TileVariantState},
            export_route::{self, ExportState},
            legend_route,
            licenses_route::{self, LicenseCatalog},
            tile_route, wmts_route,
        },
        tile_processing_worker::TileProcessingWorker,
    },
    render::{ImageFormat, Layers, RenderWorkerPool},
};
use axum::{
    Router,
    routing::{get, post},
    serve,
};
use geo::Geometry;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::broadcast::Receiver;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::cors::{Any, CorsLayer};

pub struct ServerOptions {
    pub serve_cached: bool,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub allowed_scales: Vec<f64>,
    pub max_concurrent_connections: usize,
    pub host: Ipv4Addr,
    pub port: u16,
    pub cors: bool,
    pub tile_variants: Vec<TileVariantOptions>,
    pub max_export_pixels: u64,
    pub max_parallel_exports: usize,
    pub export_abandon_grace: std::time::Duration,
    pub export_retention: std::time::Duration,
    pub licenses: LicenseCatalog,
}

pub struct TileVariantOptions {
    pub url_path: String,
    pub tile_cache_base_path: Option<PathBuf>,
    pub layers: Layers,
    pub format: ImageFormat,
    pub coverage_geometry: Option<Geometry>,
}

pub async fn start_server(
    render_worker_pool: Arc<RenderWorkerPool>,
    tile_worker: Option<TileProcessingWorker>,
    mut shutdown_rx: Receiver<()>,
    options: ServerOptions,
) -> io::Result<()> {
    let tile_variants: Vec<TileVariantState> = options
        .tile_variants
        .iter()
        .map(|variant| TileVariantState {
            tile_cache_base_path: variant.tile_cache_base_path.clone(),
            coverage_geometry: variant.coverage_geometry.clone().map(Arc::new),
            layers: variant.layers.clone(),
            format: variant.format,
        })
        .collect();

    // `/export` offers the layers the main map has; an overlay variant's set is
    // not a menu of extras, so it would mean nothing there.
    let default_render = options
        .tile_variants
        .iter()
        .find_map(|variant| match &variant.layers {
            Layers::Map(set) => Some(set.clone()),
            _ => None,
        })
        .unwrap_or_default();

    let app_state = AppState {
        render_worker_pool,
        export_state: Arc::new(ExportState::new(&options)),
        licenses: Arc::new(options.licenses),
        tile_variants: Arc::new(tile_variants),
        default_render,
        tile_worker,
        serve_cached: options.serve_cached,
        min_zoom: options.min_zoom,
        max_zoom: options.max_zoom,
        allowed_scales: options.allowed_scales.clone(),
    };

    let mut router = Router::new()
        .route("/service", get(wmts_route::service_handler))
        .route(
            "/export",
            post(export_route::post)
                .head(export_route::head)
                .get(export_route::get)
                .delete(export_route::delete),
        )
        .route("/legend", get(legend_route::get_metadata))
        .route("/legend/{id}", get(legend_route::get))
        .route("/licenses", get(licenses_route::get));

    for (variant_index, variant) in options.tile_variants.iter().enumerate() {
        let prefix = if variant.url_path == "/" {
            ""
        } else {
            &variant.url_path
        };

        router = router.route(
            &format!("{prefix}/{{zoom}}/{{x}}/{{y}}"),
            get(tile_route::get).with_state(TileRouteState {
                app_state: app_state.clone(),
                variant_index,
            }),
        );
    }

    let mut router = router.with_state(app_state);

    if options.cors {
        router = router.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
                // So a browser can read the export's attribution off the poll it
                // already makes.
                .expose_headers(Any),
        );
    }

    router = router.layer(ConcurrencyLimitLayer::new(
        options.max_concurrent_connections,
    ));

    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((options.host, options.port))).await?;

    serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
        })
        .await
}
