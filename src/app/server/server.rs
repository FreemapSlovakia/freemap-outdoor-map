use crate::{
    app::{
        server::{
            app_state::AppState,
            export_route::{self, ExportState},
            legend_route, tile_route, wmts_route,
        },
        tile_processing_worker::TileProcessingWorker,
    },
    render::RenderWorkerPool,
};
use axum::{
    Router,
    routing::{get, post},
    serve,
};
use geo::Geometry;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::broadcast;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::cors::{Any, CorsLayer};

pub async fn start_server(
    render_worker_pool: Arc<RenderWorkerPool>,
    tile_cache_base_path: Option<PathBuf>,
    tile_worker: Option<TileProcessingWorker>,
    serve_cached: bool,
    max_zoom: u8,
    limits_geometry: Option<Geometry>,
    allowed_scales: Vec<f64>,
    max_concurrent_connections: usize,
    addr: SocketAddr,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let app_state = AppState {
        render_worker_pool,
        export_state: Arc::new(ExportState::new()),
        tile_cache_base_path: Arc::new(tile_cache_base_path),
        tile_worker,
        serve_cached,
        max_zoom,
        limits_geometry: Arc::new(limits_geometry),
        allowed_scales: Arc::new(allowed_scales),
    };

    let router = Router::new()
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
        .with_state(app_state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(ConcurrencyLimitLayer::new(max_concurrent_connections));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind address");

    serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
        })
        .await
        .expect("server");
}
