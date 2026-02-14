pub(super) use feature::{Feature, FeatureError, GeomError, LegendValue};
pub(super) use image_format::ImageFormat;
pub(super) use layers::RouteTypes;
pub(crate) use legend::{LegendMeta, legend_metadata, legend_render_request};
pub(super) use render_request::RenderRequest;
pub(super) use render_worker_pool::RenderWorkerPool;
use std::path::PathBuf;

mod categories;
mod collision;
mod colors;
mod ctx;
mod draw;
mod feature;
mod image_format;
mod layer_render_error;
mod layers;
mod legend;
mod projectable;
mod regex_replacer;
mod render;
mod render_request;
mod render_worker_pool;
mod size;
mod svg_repo;
mod xyz;

pub(crate) fn set_mapping_path(path: PathBuf) {
    legend::set_mapping_path(path);
}
