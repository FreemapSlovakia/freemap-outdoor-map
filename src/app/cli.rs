use crate::render::{
    ContourCountries, FeatureLineMaskCountries, HillshadingHierarchy, ImageFormat, Layers,
    PlaceTypeOverrides, RenderLayer, WebpQuality,
};
use clap::{Parser, ValueEnum, error::ErrorKind};
use std::{collections::HashSet, net::Ipv4Addr, path::PathBuf, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileUrlPath(String);

impl TileUrlPath {
    pub const fn as_str(&self) -> &str {
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
    pub const fn layers(&self) -> &HashSet<RenderLayer> {
        &self.0
    }
}

/// A variant's output format. Lossy WebP only pays on an overlay dense enough
/// that lossless has no sparsity to exploit; a sparse one encodes smaller
/// lossless, and without the fringing a photo codec leaves around text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum TileFormat {
    #[default]
    Jpeg,
    Png,
    Webp,
    WebpLossy,
}

impl TileFormat {
    pub const fn image_format(self, quality: f32) -> ImageFormat {
        match self {
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Png => ImageFormat::Png,
            Self::Webp => ImageFormat::Webp(WebpQuality::Lossless),
            Self::WebpLossy => ImageFormat::Webp(WebpQuality::Lossy(quality)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TileVariantInput {
    pub url_path: String,
    pub coverage_geojson: Option<PathBuf>,
    pub tile_cache_base_path: Option<PathBuf>,
    pub tile_index: Option<PathBuf>,
    pub layers: Layers,
    pub format: ImageFormat,
}

impl FromStr for RenderGroup {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parsed = HashSet::new();

        // A variant may legitimately want no extras, or nothing omitted, so an
        // empty group is a group of nothing rather than a mistake.
        if value.trim().is_empty() {
            return Ok(Self(parsed));
        }

        for token in value.split(',') {
            let layer_name = token.trim();

            if layer_name.is_empty() {
                return Err(format!("render group contains an empty layer: {value}"));
            }

            let layer = RenderLayer::from_str(layer_name, true)
                .map_err(|_| format!("unknown render layer '{layer_name}'"))?;

            parsed.insert(layer);
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

    /// Path to the directory with font files (.ttf/.otf). Loaded at startup;
    /// system fonts are not consulted.
    #[arg(long, env = "MAPRENDER_FONTS_PATH")]
    pub fonts_path: PathBuf,

    /// Path to hillshading datasets.
    #[arg(long, env = "MAPRENDER_HILLSHADING_BASE_PATH")]
    pub hillshading_base_path: Option<PathBuf>,

    /// Per-country hillshading priority. Format:
    /// `<country>[:<better-csv>][;<country>[:<better-csv>]…]`. Order matters.
    /// If unset, no shading is rendered.
    #[arg(long, env = "MAPRENDER_HILLSHADING_HIERARCHY")]
    pub hillshading_hierarchy: Option<HillshadingHierarchy>,

    /// Maximum open handles per hillshading dataset, shared by all render workers.
    #[arg(
        long,
        env = "MAPRENDER_HILLSHADING_MAX_OPEN_PER_COUNTRY",
        default_value_t = 8
    )]
    pub hillshading_max_open_per_country: usize,

    /// Seconds between hillshading dataset and database pool statistics in the log.
    /// Off by default: the summaries are verbose and only wanted while chasing a stall.
    #[arg(long, env = "MAPRENDER_POOL_STATS_INTERVAL_SECS", default_value_t = 0)]
    pub pool_stats_interval_secs: u64,

    /// Country contour sources. Comma-separated country codes; the token `_` includes
    /// the global fallback source. If unset, no contours are rendered.
    #[arg(long, env = "MAPRENDER_CONTOUR_COUNTRIES")]
    pub contour_countries: Option<ContourCountries>,

    /// Countries whose hillshading is detailed enough to convey terrain feature lines
    /// (cliffs, embankments, …); those lines are masked out where the country's hillshading
    /// mask covers the tile. Comma-separated country codes; they need not appear in
    /// --hillshading-hierarchy, but a country without a hillshading dataset is ignored.
    /// If unset, nothing is masked.
    #[arg(long, env = "MAPRENDER_FEATURE_LINE_MASK_COUNTRIES")]
    pub feature_line_mask_countries: Option<FeatureLineMaskCountries>,

    /// Per-country place type remapping for place labels, for countries tagging `place=*`
    /// more finely than the style expects. Format:
    /// `<country>:<rule>[,<rule>…][;<country>:…]`, where the country is a lowercase ISO
    /// 3166-1 alpha-2 code or `*` for every country without rules of its own, and `<rule>`
    /// is `<from>[@<population>]=<to>`. The target is `z<zoom>[/<style>]` — from which zoom
    /// to label the place and in which style (xxl, xl, l, m, s, xs, xxs; the source type's
    /// own if omitted) — or `-` to not label the type in that country at all. The optional
    /// `@<population>` matches only places below that population (untagged counts as 0);
    /// several rules for one type form tiers, the lowest matching population wins and a
    /// rule without `@` takes the rest. A rule may only postpone a type, never make it
    /// appear earlier. Requires the `countries` table (see sql/countries.sql). If unset,
    /// place labels are the same everywhere.
    #[arg(long, env = "MAPRENDER_PLACE_TYPE_OVERRIDES")]
    pub place_type_overrides: Option<PlaceTypeOverrides>,

    /// Number of rendering worker threads.
    #[arg(long, env = "MAPRENDER_WORKER_COUNT")]
    pub worker_count: usize,

    /// Database connection string (e.g. <postgres://user:pass@host/dbname>).
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

    /// Replace idle database connections older than this many seconds; 0 disables.
    #[arg(
        long,
        env = "MAPRENDER_POOL_MAX_CONNECTION_AGE_SECS",
        default_value_t = 600
    )]
    pub pool_max_connection_age_secs: u64,

    /// Minimum supported zoom for serving tiles.
    #[arg(long, env = "MAPRENDER_MIN_ZOOM", default_value_t = 5)]
    pub min_zoom: u8,

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

    /// Whether each variant draws the layers the map draws by itself, aligned
    /// with tile URL paths. False makes the variant an overlay of nothing but
    /// its `--render` group.
    #[arg(long, env = "MAPRENDER_BASE_MAP", value_delimiter = ',')]
    pub base_map: Vec<bool>,

    /// Base-map layers each variant drops, groups delimited by ';' and aligned
    /// with tile URL paths. For an overlay over something that draws its own
    /// ground, an aerial image above all.
    #[arg(long, env = "MAPRENDER_OMIT", value_delimiter = ';', num_args = 1..)]
    pub omit: Vec<RenderGroup>,

    /// Output format per variant, aligned with tile URL paths.
    #[arg(long, env = "MAPRENDER_TILE_FORMAT", value_delimiter = ',')]
    pub tile_format: Vec<TileFormat>,

    /// Quality for the lossy WebP variants, 0..=100.
    #[arg(long, env = "MAPRENDER_WEBP_QUALITY", default_value_t = 80.0)]
    pub webp_quality: f32,

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

    /// Tile index files aligned with tile URL paths.
    #[arg(long, env = "MAPRENDER_INDEX", value_delimiter = ',')]
    pub index: Vec<PathBuf>,

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

    /// Optional overrides for `GET /licenses`, which otherwise answers from each
    /// dataset's own `<hillshading-base-path>/<key>/attribution.json` plus a built-in
    /// entry for `osm`. A JSON object keyed by code (`shading:<key>`,
    /// `contours:<key>`, where `<key>` is a hillshading-hierarchy / contour-country
    /// key or `_` for a global fallback), each value a list of
    /// `{"title": "…", "url": "…"}` with `url` optional. Use it for a code with no
    /// dataset directory, or to correct one without touching the data volume; it
    /// replaces whatever the dataset said. Titles are not localized.
    #[arg(long, env = "MAPRENDER_LICENSES")]
    pub licenses: Option<PathBuf>,

    /// Maximum total pixel area allowed for a single export request. The
    /// estimated pixel count is `bbox_width_px * bbox_height_px` at the
    /// requested zoom (scale is ignored — it does not significantly affect
    /// rendering cost); requests exceeding this are rejected upfront.
    #[arg(
        long,
        env = "MAPRENDER_MAX_EXPORT_PIXELS",
        default_value_t = 10_000_000
    )]
    pub max_export_pixels: u64,

    /// Maximum number of export render jobs allowed to run in parallel.
    /// Additional exports wait in a queue.
    #[arg(long, env = "MAPRENDER_MAX_PARALLEL_EXPORTS", default_value_t = 1)]
    pub max_parallel_exports: usize,

    /// Abandon a queued export if no client (HEAD/GET) has been
    /// actively polling it for this many seconds. Also covers the gap
    /// between POST and the first poll.
    #[arg(
        long,
        env = "MAPRENDER_EXPORT_ABANDON_GRACE_SECS",
        default_value_t = 30
    )]
    pub export_abandon_grace_secs: u64,

    /// Keep a finished export this many seconds before dropping the job and its
    /// temporary file. Only the client that failed to delete its own job needs
    /// it, so it is generous rather than tight.
    #[arg(long, env = "MAPRENDER_EXPORT_RETENTION_SECS", default_value_t = 900)]
    pub export_retention_secs: u64,
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

        if !(0.0..=100.0).contains(&self.webp_quality) {
            return Err(format!(
                "webp-quality {} is outside 0..=100",
                self.webp_quality
            ));
        }

        if self.min_zoom > self.max_zoom {
            return Err(format!(
                "min-zoom {} is greater than max-zoom {}",
                self.min_zoom, self.max_zoom
            ));
        }

        for variant in self.tile_variant_inputs()? {
            // An overlay leaves most of the surface unpainted; an opaque format
            // renders that as solid black rather than as nothing.
            if !variant.layers.is_whole_map() && !variant.format.has_alpha() {
                return Err(format!(
                    "tile URL path '{}' is an overlay, so it needs an alpha-capable --tile-format",
                    variant.url_path
                ));
            }
        }

        if let Some(hierarchy) = self.hillshading_hierarchy.as_ref() {
            let keys: HashSet<&str> = hierarchy.entries().iter().map(|e| e.country).collect();

            for entry in hierarchy.entries() {
                for better in &entry.better {
                    if !keys.contains(better) {
                        return Err(format!(
                            "hillshading-hierarchy entry '{}' references unknown better-country '{better}'",
                            entry.country
                        ));
                    }
                }
            }

            if let Some(contour_countries) = self.contour_countries.as_ref() {
                for entry in contour_countries.entries() {
                    if !keys.contains(entry.country) {
                        return Err(format!(
                            "contour-countries country '{}' is not a key in hillshading-hierarchy",
                            entry.country
                        ));
                    }
                }
            }
        }

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
        let index_by_variant = expand_optional_by_variant(&self.index, variants_len, "--index")?;
        let base_by_variant = expand_optional_by_variant(&self.base_map, variants_len, "--base-map")?;
        let omit_by_variant = expand_optional_by_variant(&self.omit, variants_len, "--omit")?;
        let format_by_variant =
            expand_optional_by_variant(&self.tile_format, variants_len, "--tile-format")?;

        let mut result = Vec::with_capacity(variants_len);

        for i in 0..variants_len {
            let layers = Layers {
                base_map: base_by_variant[i].unwrap_or(true),
                add: render_by_variant[i].layers().clone(),
                omit: omit_by_variant[i]
                    .as_ref()
                    .map(|group| group.layers().clone())
                    .unwrap_or_default(),
            };

            layers
                .validate()
                .map_err(|err| format!("tile URL path '{}': {err}", self.tile_url_path[i].as_str()))?;

            result.push(TileVariantInput {
                url_path: self.tile_url_path[i].as_str().to_string(),
                coverage_geojson: coverage_by_variant[i].clone(),
                tile_cache_base_path: cache_by_variant[i].clone(),
                tile_index: index_by_variant[i].clone(),
                layers,
                format: format_by_variant[i]
                    .unwrap_or_default()
                    .image_format(self.webp_quality),
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
