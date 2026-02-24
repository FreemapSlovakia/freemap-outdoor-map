use crate::app::{
    cli::{Cli, TileVariantInput},
    server::{ServerOptions, TileVariantOptions, start_server},
    tile_invalidation,
    tile_processing_worker::TileProcessingWorker,
    tile_processor::TileProcessingConfig,
};
use crate::render::{RenderWorkerPool, set_mapping_path};
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
    path::{Path, PathBuf},
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

    let cli = Cli::parse_checked();
    set_mapping_path(cli.mapping_path.clone());

    let tile_variants = match build_tile_variants(&cli) {
        Ok(config) => config,
        Err(err) => panic!("invalid tile route configuration: {err}"),
    };

    let mut tile_cache_base_paths = Vec::<PathBuf>::new();
    for variant in &tile_variants {
        if let Some(path) = variant.tile_cache_base_path.as_ref()
            && !tile_cache_base_paths.contains(path)
        {
            tile_cache_base_paths.push(path.clone());
        }
    }

    let render_worker_pool = {
        let connection_pool = r2d2::Pool::builder()
            .max_size(cli.pool_max_size)
            .build(PostgresConnectionManager::new(
                Config::from_str(&cli.database_url).expect("parse database url"),
                NoTls,
            ))
            .expect("build db pool");

        Arc::new(RenderWorkerPool::new(
            connection_pool,
            cli.worker_count,
            Arc::from(cli.svg_base_path),
            Arc::from(cli.hillshading_base_path),
        ))
    };

    let mut tile_processing_worker = None;
    let mut tile_invalidation_watcher = None;

    if !tile_cache_base_paths.is_empty() {
        let processing_config = TileProcessingConfig {
            tile_cache_base_paths,
            tile_index: cli.index.clone(),
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
        eprintln!("imposm watcher disabled: missing --tile-cache-base-path");
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

    if let Err(err) = rt.block_on(start_server(
        render_worker_pool.clone(),
        tile_processing_worker_for_server,
        shutdown_tx.subscribe(),
        ServerOptions {
            serve_cached: cli.serve_cached,
            max_zoom: cli.max_zoom,
            allowed_scales: cli.allowed_scales,
            max_concurrent_connections: cli.max_concurrent_connections,
            host: cli.host,
            port: cli.port,
            cors: cli.cors,
            tile_variants,
        },
    )) {
        eprintln!("Server stopped with error: {err}");
    }

    shutdown_tile_workers(&tile_invalidation_watcher, &tile_processing_worker);

    println!("Stopping render worker pool.");
    render_worker_pool.shutdown();
    println!("Render worker pool stopped.");
}

fn build_tile_variants(cli: &Cli) -> Result<Vec<TileVariantOptions>, String> {
    let variant_inputs = cli.tile_variant_inputs()?;

    variant_inputs
        .into_iter()
        .map(tile_variant_input_to_server_variant)
        .collect()
}

fn tile_variant_input_to_server_variant(
    variant: TileVariantInput,
) -> Result<TileVariantOptions, String> {
    let coverage_geometry =
        match variant.coverage_geojson.as_ref() {
            Some(path) => Some(load_geometry_from_geojson(path).map_err(|err| {
                format!("failed to load coverage geojson {}: {err}", path.display())
            })?),
            None => None,
        };

    Ok(TileVariantOptions {
        url_path: variant.url_path,
        tile_cache_base_path: variant.tile_cache_base_path,
        render: variant.render,
        coverage_geometry,
    })
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
        Err("failed to project some coverage coordinates to EPSG:3857".into())
    } else {
        Ok(geometry)
    }
}
