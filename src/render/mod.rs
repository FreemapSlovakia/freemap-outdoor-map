pub(crate) use coverage::{TileCoverageRelation, tile_touches_coverage};
pub(super) use feature::{Feature, FeatureError, GeomError, LegendValue};
pub(super) use image_format::ImageFormat;
pub(crate) use legend::{LegendMeta, LegendMode, legend_metadata, legend_render_request};
pub(crate) use render_config::{ContourCountries, HillshadingHierarchy, RenderConfig};
pub(super) use render_request::{
    CustomLayer, CustomLayerOrder, Decorations, RenderLayer, RenderRequest,
};
pub(super) use render_worker_pool::RenderWorkerPool;
pub(super) use xyz::bbox_size_in_pixels;
use std::path::PathBuf;

mod categories;
mod collision;
mod colors;
mod coverage;
mod ctx;
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

pub(crate) fn set_mapping_path(path: PathBuf) {
    legend::set_mapping_path(path);
}

pub(crate) fn set_fonts_path(path: PathBuf) {
    draw::font_system::set_fonts_path(path);
}
