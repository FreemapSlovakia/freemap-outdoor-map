use crate::{
    app::tile_variants::TileVariants,
    render::{
        ContourCountries, FeatureLineMaskCountries, HillshadingHierarchy, PlaceTypeOverrides,
    },
};
use clap::{Parser, error::ErrorKind};
use std::{collections::HashSet, net::Ipv4Addr, path::PathBuf};

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

    /// Every tile route, one per `;`-separated entry, lines allowed. An entry is
    /// a URL path followed by space-separated fields in any order: a format
    /// (`jpeg` by default, or `png`, `webp`, `webp-lossy`, the lossy ones taking
    /// an optional `=<quality>` — `jpeg=85`, default 90 and 80), `overlay` to leave
    /// out the layers the map draws by itself, `+<layers>` to add extras,
    /// `-<layers>` to drop base layers, and `cache=`, `index=`, `coverage=`.
    /// A layer is either an extra or part of the map, never both, so naming one
    /// in the wrong list is an error rather than a silent no-op.
    #[arg(long, env = "MAPRENDER_TILE_VARIANTS", default_value = "/")]
    pub tile_variants: TileVariants,

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
        // Whole numbers because the tile index packs a scale into one byte with
        // the top bit reserved, so a fractional scale would collide with its
        // neighbour. The ceiling is far below that bit: a tile side is 256 times
        // the scale, and the encoders and buffers downstream give out long
        // before 127 would.
        if let Some(scale) = self
            .allowed_scales
            .iter()
            .find(|scale| **scale < 1.0 || **scale > 8.0 || scale.fract() != 0.0)
        {
            return Err(format!(
                "allowed-scales must be whole numbers 1..=8, got {scale}"
            ));
        }

        if self.min_zoom > self.max_zoom {
            return Err(format!(
                "min-zoom {} is greater than max-zoom {}",
                self.min_zoom, self.max_zoom
            ));
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

}
