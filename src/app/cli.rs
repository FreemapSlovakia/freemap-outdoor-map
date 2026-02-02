use clap::Parser;
use std::{net::Ipv4Addr, path::PathBuf};

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Cli {
    /// Path to the directory with symbol SVGs.
    #[arg(long, env = "MAPRENDER_SVG_BASE_PATH")]
    pub svg_base_path: PathBuf,

    /// Path to hillshading datasets.
    #[arg(long, env = "MAPRENDER_HILLSHADING_BASE_PATH")]
    pub hillshading_base_path: PathBuf,

    /// Number of rendering worker threads.
    #[arg(long, env = "MAPRENDER_WORKER_COUNT")]
    pub worker_count: usize,

    /// Database connection string (e.g. postgres://user:pass@host/dbname).
    #[arg(long, env = "MAPRENDER_DATABASE_URL")]
    pub database_url: String,

    /// HTTP bind address.
    #[arg(long, env = "MAPRENDER_HOST", default_value_t = Ipv4Addr::LOCALHOST)]
    pub host: Ipv4Addr,

    /// HTTP bind port.
    #[arg(long, env = "MAPRENDER_PORT", default_value_t = 3050)]
    pub port: u16,

    /// Maximum concurrent HTTP connections.
    #[arg(
        long,
        env = "MAPRENDER_MAX_CONCURRENT_CONNECTIONS",
        default_value_t = 4096
    )]
    pub max_concurrent_connections: usize,

    /// Database pool max size.
    #[arg(long, env = "MAPRENDER_POOL_MAX_SIZE")]
    pub pool_max_size: u32,

    /// Maximum supported zoom for serving tiles.
    #[arg(long, env = "MAPRENDER_MAX_ZOOM", default_value_t = 20)]
    pub max_zoom: u8,

    /// Allowed tile scales (e.g. 1,2,3).
    #[arg(
        long,
        env = "MAPRENDER_ALLOWED_SCALES",
        value_delimiter = ',',
        default_value = "1"
    )]
    pub allowed_scales: Vec<f64>,

    /// Optional polygon geojson limiting requested tiles.
    #[arg(long, env = "MAPRENDER_LIMITS_GEOJSON")]
    pub limits_geojson: Option<PathBuf>,

    /// Mask geojson polygon file
    #[arg(long, env = "MAPRENDER_MASK_GEOJSON")]
    pub mask_geojson: Option<PathBuf>,

    /// Base directory for cached tiles.
    #[arg(long, env = "MAPRENDER_TILE_CACHE_ROOT")]
    pub tile_cache_root: Option<PathBuf>,

    /// Serve cached tiles from the filesystem.
    #[arg(
        long,
        env = "MAPRENDER_SERVE_CACHED",
        default_value_t = true,
        action = clap::ArgAction::Set
    )]
    pub serve_cached: bool,

    /// Base directory to watch for expire .tile updates.
    #[arg(long, env = "MAPRENDER_EXPIRES_BASE_PATH")]
    pub expires_base_path: Option<PathBuf>,

    /// Lowest zoom to invalidate for parent tiles.
    #[arg(long, env = "MAPRENDER_INVALIDATE_MIN_ZOOM", default_value_t = 0)]
    pub invalidate_min_zoom: u8,

    /// Zoom level used for .index files (e.g. 14).
    #[arg(long, env = "MAPRENDER_INDEX_ZOOM", default_value_t = 14)]
    pub index_zoom: u8,
}
