use crate::app::{
    cli::Cli, server::start_server, tile_invalidation,
    tile_processing_worker::TileProcessingWorker, tile_processor::TileProcessingConfig,
};
use crate::render::{RenderWorkerPool, set_mapping_path};
use clap::Parser;
use dotenvy::dotenv;
use geo::{Coord, Geometry, MapCoordsInPlace};
use geojson::GeoJson;
use postgres::{Config, NoTls};
use proj::Proj;
use r2d2_postgres::PostgresConnectionManager;
use std::{
    cell::Cell,
    fs::File,
    io::BufReader,
    net::SocketAddr,
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex},
};
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal as unix_signal};
use tokio::sync::broadcast;

pub(crate) fn start() {
    dotenv().ok();

    tracy_client::Client::start();

    let cli = Cli::parse();
    set_mapping_path(cli.mapping_path.clone());

    let render_worker_pool = {
        let connection_pool = r2d2::Pool::builder()
            .max_size(cli.pool_max_size)
            .build(PostgresConnectionManager::new(
                Config::from_str(&cli.database_url).expect("parse database url"),
                NoTls,
            ))
            .expect("build db pool");

        let mask_geometry = cli
            .mask_geojson
            .map(|path| match load_geometry_from_geojson(&path) {
                Ok(g) => g,
                Err(err) => panic!("failed to load mask geojson {}: {err}", path.display()),
            });

        Arc::new(RenderWorkerPool::new(
            connection_pool,
            cli.worker_count,
            Arc::from(cli.svg_base_path),
            Arc::from(cli.hillshading_base_path),
            mask_geometry,
        ))
    };

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

    let mut tile_processing_worker = None;
    let mut tile_invalidation_watcher = None;

    if let Some(tile_cache_base_path) = cli.tile_cache_base_path.clone() {
        let processing_config = TileProcessingConfig {
            tile_cache_base_path,
            tile_index: cli.index,
            invalidate_min_zoom: cli.invalidate_min_zoom,
        };

        println!("Starting tile processing worker");
        let worker = TileProcessingWorker::new(processing_config);
        tile_processing_worker = Some(worker.clone());

        if let Some(watch_base) = cli.expires_base_path.clone() {
            println!("Processing existing tile expiration files");
            tile_invalidation::process_existing_expiration_files(watch_base.as_ref(), &worker);

            println!("Starting tile invalidation watcher");
            tile_invalidation_watcher = Some(tile_invalidation::start_watcher(
                watch_base.as_ref(),
                worker,
            ));
        }
    } else if cli.expires_base_path.is_some() {
        eprintln!("imposm watcher disabled: missing --tile-base-path");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");

    let tile_processing_worker_for_server = tile_processing_worker.clone();

    let tile_processing_worker = Arc::new(Mutex::new(tile_processing_worker));
    let tile_invalidation_watcher = Arc::new(Mutex::new(tile_invalidation_watcher));

    let (shutdown_tx, _) = broadcast::channel(1);

    rt.spawn({
        let shutdown_tx_signal = shutdown_tx.clone();
        let tile_processing_worker = tile_processing_worker.clone();
        let tile_invalidation_watcher = tile_invalidation_watcher.clone();

        async move {
            shutdown_signal(shutdown_tx_signal).await;

            let result = tokio::task::spawn_blocking(move || {
                shutdown_tile_workers(&tile_invalidation_watcher, &tile_processing_worker);
            })
            .await;

            if let Err(err) = result {
                eprintln!("Error joining: {err}");
            }
        }
    });

    rt.block_on(start_server(
        render_worker_pool.clone(),
        cli.tile_cache_base_path.clone(),
        tile_processing_worker_for_server,
        cli.serve_cached,
        cli.max_zoom,
        limits_geometry,
        cli.allowed_scales.clone(),
        cli.max_concurrent_connections,
        SocketAddr::from((cli.host, cli.port)),
        shutdown_tx.subscribe(),
        cli.cors,
    ));

    shutdown_tile_workers(&tile_invalidation_watcher, &tile_processing_worker);

    println!("Stopping render worker pool.");
    render_worker_pool.shutdown();
    println!("Render worker pool stopped.");
}

async fn shutdown_signal(shutdown_tx: broadcast::Sender<()>) {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        let mut sigterm = unix_signal(SignalKind::terminate()).expect("install SIGTERM handler");
        sigterm.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    if let Err(err) = shutdown_tx.send(()) {
        eprintln!("Error sending shutdown signal: {err}");
    }
}

fn shutdown_tile_workers(
    tile_invalidation_watcher: &Arc<Mutex<Option<tile_invalidation::TileInvalidationWatcher>>>,
    tile_processing_worker: &Arc<Mutex<Option<TileProcessingWorker>>>,
) {
    let watcher = tile_invalidation_watcher.lock().unwrap().take();
    let worker = tile_processing_worker.lock().unwrap().take();

    if let Some(watcher) = watcher {
        println!("Stopping tile invalidation watcher.");
        watcher.shutdown();
        println!("Tile invalidation watcher stopped.");
    }

    if let Some(worker) = worker {
        println!("Stopping tile processing worker.");
        worker.shutdown();
        println!("Tile processing worker stopped.");
    }
}

pub fn load_geometry_from_geojson(path: &Path) -> Result<Geometry, String> {
    let file = File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;

    let reader = BufReader::new(file);

    let geojson: GeoJson = serde_json::from_reader(reader)
        .map_err(|err| format!("parse {}: {err}", path.display()))?;

    let mut geometry: Geometry = Geometry::try_from(geojson)
        .map_err(|err| format!("convert {} to geo geometry: {err}", path.display()))?;

    let proj = Proj::new_known_crs("EPSG:4326", "EPSG:3857", None)
        .map_err(|err| format!("failed to create 4326->3857 projection: {err}"))?;

    let failed = Cell::new(false);

    geometry.map_coords_in_place(|coord: Coord| match proj.convert((coord.x, coord.y)) {
        Ok((x, y)) => Coord { x, y },
        Err(_) => {
            failed.set(true);
            coord
        }
    });

    if failed.get() {
        Err("failed to project some mask coordinates to EPSG:3857".into())
    } else {
        Ok(geometry)
    }
}
