use crate::{
    app::{
        server::{
            app_state::{AppState, TileRouteState, TileVariantState},
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
    pub max_zoom: u8,
    pub allowed_scales: Vec<f64>,
    pub max_concurrent_connections: usize,
    pub host: Ipv4Addr,
    pub port: u16,
    pub cors: bool,
    pub tile_variants: Vec<TileVariantOptions>,
}

pub struct TileVariantOptions {
    pub url_path: String,
    pub tile_cache_base_path: Option<PathBuf>,
    pub render: std::collections::HashSet<RenderLayer>,
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
            render: variant.render.iter().copied().collect(),
        })
        .collect();

    let default_render = tile_variants
        .first()
        .map(|variant| variant.render.to_owned())
        .unwrap_or_default();

    let app_state = AppState {
        render_worker_pool,
        export_state: Arc::new(ExportState::new()),
        tile_variants: Arc::new(tile_variants),
        default_render,
        tile_worker,
        serve_cached: options.serve_cached,
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
        .route("/legend/{id}", get(legend_route::get));

    for (variant_index, variant) in options.tile_variants.iter().enumerate() {
        let route_path = format!(
            "{}/{{zoom}}/{{x}}/{{y}}",
            if variant.url_path == "/" {
                ""
            } else {
                &variant.url_path
            }
        );

        router = router.route(
            &route_path,
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
                .allow_headers(Any),
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
