use crate::{
    app::{
        server::{
            app_state::AppState,
            export_route::{self, ExportState},
            legend_route, tile_route, wmts_route,
        },
        tile_processing_worker::TileProcessingWorker,
    },
    render::{RenderLayer, RenderWorkerPool},
};
use axum::{
    Router,
    routing::{get, post},
    serve,
};
use geo::Geometry;
use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::broadcast::Receiver;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::cors::{Any, CorsLayer};

pub struct ServerOptions {
    pub serve_cached: bool,
    pub max_zoom: u8,
    pub tile_cache_base_path: Option<PathBuf>,
    pub allowed_scales: Vec<f64>,
    pub render: HashSet<RenderLayer>,
    pub max_concurrent_connections: usize,
    pub host: Ipv4Addr,
    pub port: u16,
    pub cors: bool,
    pub limits_geometry: Option<Geometry>,
}

pub async fn start_server(
    render_worker_pool: Arc<RenderWorkerPool>,
    tile_worker: Option<TileProcessingWorker>,
    mut shutdown_rx: Receiver<()>,
    options: ServerOptions,
) {
    let app_state = AppState {
        render_worker_pool,
        export_state: Arc::new(ExportState::new()),
        tile_cache_base_path: options.tile_cache_base_path.clone(),
        tile_worker,
        serve_cached: options.serve_cached,
        max_zoom: options.max_zoom,
        limits_geometry: options.limits_geometry,
        allowed_scales: options.allowed_scales.clone(),
        render: options.render.iter().copied().collect(),
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
        .route("/{zoom}/{x}/{y}", get(tile_route::get))
        .route("/legend", get(legend_route::get_metadata))
        .route("/legend/{id}", get(legend_route::get))
        .with_state(app_state);

    if options.cors {
        router = router.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );
    }

    router = router.layer(ConcurrencyLimitLayer::new(
        options.max_concurrent_connections,
    ));

    let listener = tokio::net::TcpListener::bind(SocketAddr::from((options.host, options.port)))
        .await
        .expect("bind address");

    serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
        })
        .await
        .expect("server");
}
