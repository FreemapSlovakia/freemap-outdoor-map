use crate::{
    app::{server::export_route::ExportState, tile_processing_worker::TileProcessingWorker},
    render::{RenderLayer, RenderWorkerPool},
};
use geo::Geometry;
use std::{collections::HashSet, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(crate) struct TileVariantState {
    pub(crate) tile_cache_base_path: Option<PathBuf>,
    pub(crate) coverage_geometry: Option<Arc<Geometry>>,
    pub(crate) render: HashSet<RenderLayer>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) render_worker_pool: Arc<RenderWorkerPool>,
    pub(crate) export_state: Arc<ExportState>,
    pub(crate) tile_variants: Arc<Vec<TileVariantState>>,
    pub(crate) default_render: HashSet<RenderLayer>,
    pub(crate) tile_worker: Option<TileProcessingWorker>,
    pub(crate) serve_cached: bool,
    pub(crate) max_zoom: u8,
    pub(crate) allowed_scales: Vec<f64>,
}

#[derive(Clone)]
pub(crate) struct TileRouteState {
    pub(crate) app_state: AppState,
    pub(crate) variant_index: usize,
}
