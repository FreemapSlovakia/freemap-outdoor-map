use crate::render::RenderLayer;
use clap::{Parser, ValueEnum, error::ErrorKind};
use std::{collections::HashSet, net::Ipv4Addr, path::PathBuf, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileUrlPath(String);

impl TileUrlPath {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl FromStr for TileUrlPath {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();

        if trimmed.is_empty() {
            return Err("tile URL path cannot be empty".into());
        }

        if !trimmed.starts_with('/') {
            return Err(format!("tile URL path must start with '/': {trimmed}"));
        }

        if trimmed == "/" {
            Ok(Self("/".to_string()))
        } else {
            Ok(Self(trimmed.trim_end_matches('/').to_string()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderGroup(HashSet<RenderLayer>);

impl RenderGroup {
    pub fn layers(&self) -> &HashSet<RenderLayer> {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub struct TileVariantInput {
    pub url_path: String,
    pub coverage_geojson: Option<PathBuf>,
    pub tile_cache_base_path: Option<PathBuf>,
    pub render: HashSet<RenderLayer>,
}

impl FromStr for RenderGroup {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parsed = HashSet::new();

        for token in value.split(',') {
            let layer_name = token.trim();

            if layer_name.is_empty() {
                return Err(format!("render group contains an empty layer: {value}"));
            }

            let layer = RenderLayer::from_str(layer_name, true)
                .map_err(|_| format!("unknown render layer '{layer_name}'"))?;

            parsed.insert(layer);
        }

        if parsed.is_empty() {
            return Err(format!("render group cannot be empty: {value}"));
        }

        Ok(Self(parsed))
    }
}

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

    /// URL path prefixes for tile routes (e.g. /,/kst).
    #[arg(
        long,
        env = "MAPRENDER_TILE_URL_PATH",
        value_delimiter = ',',
        default_value = "/"
    )]
    pub tile_url_path: Vec<TileUrlPath>,

    /// Coverage geojson polygon files aligned with tile URL paths.
    #[arg(long, env = "MAPRENDER_COVERAGE_GEOJSON", value_delimiter = ',')]
    pub coverage_geojson: Vec<PathBuf>,

    /// Cache base directories aligned with tile URL paths.
    #[arg(long, env = "MAPRENDER_TILE_CACHE_BASE_PATH", value_delimiter = ',')]
    pub tile_cache_base_path: Vec<PathBuf>,

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

    /// Tile index file
    #[arg(long, env = "MAPRENDER_INDEX")]
    pub index: Option<PathBuf>,

    /// Path to the imposm mapping YAML.
    #[arg(long, env = "MAPRENDER_MAPPING_PATH", default_value = "mapping.yaml")]
    pub mapping_path: PathBuf,

    /// Enable cors
    #[arg(
        long,
        env = "MAPRENDER_CORS",
        default_value_t = false,
        action = clap::ArgAction::Set
    )]
    pub cors: bool,

    #[arg(
        long,
        env = "MAPRENDER_RENDER",
        value_delimiter = ';',
        num_args = 1..,
    )]
    /// Render layers per tile URL path group (items delimited by ',', groups by ';').
    pub render: Vec<RenderGroup>,
}

impl Cli {
    pub fn parse_checked() -> Self {
        let cli = Self::parse();

        if let Err(err) = cli.validate() {
            clap::Error::raw(ErrorKind::ValueValidation, err).exit();
        }

        cli
    }

    fn validate(&self) -> Result<(), String> {
        if self.tile_url_path.is_empty() {
            return Err("at least one tile URL path is required".into());
        }

        let variants_len = self.tile_url_path.len();
        let unique_path_count = self.tile_url_path.iter().collect::<HashSet<_>>().len();

        if unique_path_count != variants_len {
            return Err("tile URL paths must be unique".into());
        }

        self.tile_variant_inputs()?;

        Ok(())
    }

    pub fn tile_variant_inputs(&self) -> Result<Vec<TileVariantInput>, String> {
        let variants_len = self.tile_url_path.len();
        let render_by_variant = expand_required_by_variant(&self.render, variants_len, "--render")?;
        let coverage_by_variant =
            expand_optional_by_variant(&self.coverage_geojson, variants_len, "--coverage-geojson")?;
        let cache_by_variant = expand_optional_by_variant(
            &self.tile_cache_base_path,
            variants_len,
            "--tile-cache-base-path",
        )?;

        let mut result = Vec::with_capacity(variants_len);

        for i in 0..variants_len {
            result.push(TileVariantInput {
                url_path: self.tile_url_path[i].as_str().to_string(),
                coverage_geojson: coverage_by_variant[i].clone(),
                tile_cache_base_path: cache_by_variant[i].clone(),
                render: render_by_variant[i].layers().clone(),
            });
        }

        Ok(result)
    }
}

fn validate_optional_count(count: usize, variants_len: usize, name: &str) -> Result<(), String> {
    if count == 0 || count == 1 || count == variants_len {
        Ok(())
    } else {
        Err(format!(
            "{name} count ({count}) must be 0, 1, or match --tile-url-path count ({variants_len})"
        ))
    }
}

fn validate_required_count(count: usize, variants_len: usize, name: &str) -> Result<(), String> {
    if count == 1 || count == variants_len {
        Ok(())
    } else {
        Err(format!(
            "{name} count ({count}) must be 1 or match --tile-url-path count ({variants_len})"
        ))
    }
}

fn expand_optional_by_variant<T: Clone>(
    values: &[T],
    variants_len: usize,
    name: &str,
) -> Result<Vec<Option<T>>, String> {
    validate_optional_count(values.len(), variants_len, name)?;

    Ok(match values.len() {
        0 => vec![None; variants_len],
        1 => vec![Some(values[0].clone()); variants_len],
        _ => values.iter().cloned().map(Some).collect(),
    })
}

fn expand_required_by_variant<T: Clone>(
    values: &[T],
    variants_len: usize,
    name: &str,
) -> Result<Vec<T>, String> {
    validate_required_count(values.len(), variants_len, name)?;

    Ok(match values.len() {
        1 => vec![values[0].clone(); variants_len],
        _ => values.to_vec(),
    })
}
