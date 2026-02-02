use crate::app::{
    cli::Cli, server::start_server, tile_invalidation,
    tile_processing_worker::TileProcessingWorker, tile_processor::TileProcessingConfig,
};
use crate::render::RenderWorkerPool;
use clap::Parser;
use dotenvy::dotenv;
use geo::{Coord, Geometry, MapCoordsInPlace};
use geojson::GeoJson;
use postgres::{Config, NoTls};
use proj::Proj;
use r2d2_postgres::PostgresConnectionManager;
use std::{
    cell::Cell, fs::File, io::BufReader, net::SocketAddr, path::Path, str::FromStr, sync::Arc,
};

pub(crate) fn start() {
    dotenv().ok();

    tracy_client::Client::start();

    let cli = Cli::parse();

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

        RenderWorkerPool::new(
            connection_pool,
            cli.worker_count,
            Arc::from(cli.svg_base_path),
            Arc::from(cli.hillshading_base_path),
            mask_geometry,
        )
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

    if let Some(tile_cache_root) = cli.tile_cache_root.clone() {
        let processing_config = TileProcessingConfig {
            tile_cache_root,
            index_zoom: cli.index_zoom,
            max_zoom: cli.max_zoom,
            invalidate_min_zoom: cli.invalidate_min_zoom,
        };

        println!("Starting tile processing worker");
        let worker = TileProcessingWorker::new(processing_config);
        tile_processing_worker = Some(worker.clone());

        if let Some(watch_base) = cli.expires_base_path.clone() {
            println!("Processing existing tile expiration files");
            tile_invalidation::process_existing_expiration_files(watch_base.as_ref(), &worker);

            println!("Starting tile invalidation watcher");
            tile_invalidation::start_watcher(watch_base.as_ref(), worker);
        }
    } else if cli.expires_base_path.is_some() {
        eprintln!("imposm watcher disabled: missing --tile-base-path");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");

    rt.block_on(start_server(
        render_worker_pool,
        cli.tile_cache_root.clone(),
        tile_processing_worker,
        cli.serve_cached,
        cli.max_zoom,
        limits_geometry,
        cli.allowed_scales.clone(),
        cli.max_concurrent_connections,
        SocketAddr::from((cli.host, cli.port)),
    ));
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
