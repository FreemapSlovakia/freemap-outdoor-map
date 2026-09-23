pub use attribution::{ATTRIBUTION_HEADER, Attribution, FALLBACK_KEY, OSM as OSM_CODE};
pub use coverage::{TileCoverageRelation, tile_touches_coverage};
pub use feature::{Feature, FeatureError, GeomError, LegendValue};
pub use image_format::{ImageFormat, WebpQuality};
pub use layers::key_enabled;
pub use legend::{LegendMode, legend_metadata, legend_render_request};
pub use render_config::{
    ContourCountries, FeatureLineMaskCountries, HillshadingHierarchy, PlaceTypeOverride,
    PlaceTypeOverrides, RenderConfig,
};
pub use render_request::{
    AttributionDecoration, CustomLayer, CustomLayerOrder, Decorations, Glow, LabelStyle, Layers,
    RenderLayer, RenderRequest,
};
pub use render_worker_pool::RenderWorkerPool;
pub use xyz::bbox_size_in_pixels;
use std::path::PathBuf;

mod attribution;
mod categories;
mod collision;
mod colors;
mod coverage;
mod ctx;
mod db_pool_stats;
mod draw;
mod feature;
mod image_format;
mod layer_render_error;
mod layers;
mod legend;
mod projectable;
mod regex_replacer;
mod render_config;
mod render_request;
mod render_worker_pool;
mod renderer;
mod size;
mod svg_repo;
mod xyz;

pub fn set_mapping_path(path: PathBuf) {
    legend::set_mapping_path(path);
}

pub fn set_fonts_path(path: PathBuf) {
    draw::font_system::set_fonts_path(path);
}
