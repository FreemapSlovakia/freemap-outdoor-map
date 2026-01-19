use crate::{
    app::{
        app_state::AppState, cli::Cli, export, render_worker_pool::RenderWorkerPool, service,
        tile_invalidation, tiles,
    },
    render::load_geometry_from_geojson,
};
use axum::{
    Router,
    routing::{get, post},
    serve,
};
use clap::Parser;
use dotenvy::dotenv;
use postgres::{Config, NoTls};
use r2d2_postgres::PostgresConnectionManager;
use std::{net::SocketAddr, str::FromStr, sync::Arc};
use tower::limit::ConcurrencyLimitLayer;

pub(crate) async fn start() {
    dotenv().ok();

    tracy_client::Client::start();

    let cli = Cli::parse();

    let pg_config = Config::from_str(&cli.database_url).expect("parse database url");

    let manager = PostgresConnectionManager::new(pg_config, NoTls);

    let connection_pool = r2d2::Pool::builder()
        .max_size(cli.pool_max_size)
        .build(manager)
        .expect("build db pool");

    let mask_geometry = cli
        .mask_geojson
        .map(|path| match load_geometry_from_geojson(&path) {
            Ok(g) => g,
            Err(err) => panic!("failed to load mask geojson {}: {err}", path.display()),
        });

    let worker_pool = RenderWorkerPool::new(
        connection_pool,
        cli.worker_count,
        Arc::from(cli.svg_base_path),
        Arc::from(cli.hillshading_base_path),
        mask_geometry,
    );

    let limits_geometry = cli
        .limits_geojson
        .as_ref()
        .map(|path| match load_geometry_from_geojson(path) {
            Ok(geometry) => geometry,
            Err(err) => panic!(
                "failed to load limits geojson {}: {err}",
                path.to_string_lossy()
            ),
        });

    let app_state = AppState::new(
        worker_pool,
        cli.tile_base_path.clone(),
        cli.index_zoom,
        cli.max_zoom,
        limits_geometry,
        cli.allowed_scales.clone(),
    );

    if let Some(watch_base) = cli.expires_base_path.clone() {
        if let Some(tile_base_path) = cli.tile_base_path.clone() {
            let invalidation_config = tile_invalidation::InvalidationConfig {
                watch_base,
                tile_base_path,
                parent_min_zoom: cli.invalidate_min_zoom,
                index_zoom: cli.index_zoom,
                max_zoom: cli.max_zoom,
            };
            tile_invalidation::process_recovery_files(&invalidation_config);
            tile_invalidation::start_watcher(invalidation_config);
        } else {
            eprintln!("imposm watcher disabled: missing --tile-base-path");
        }
    }

    let app = Router::new()
        .route("/service", get(service::service_handler))
        .route(
            "/export",
            post(export::export_post)
                .head(export::export_head)
                .get(export::export_get)
                .delete(export::export_delete),
        )
        .route("/{zoom}/{x}/{y}", get(tiles::tile_get))
        .with_state(app_state)
        .layer(ConcurrencyLimitLayer::new(cli.max_concurrent_connections));

    let addr = SocketAddr::from((cli.host, cli.port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind address");

    serve(listener, app).await.expect("server");
}
