use crate::{
    app::{server::export_route::ExportState, tile_processing_worker::TileProcessingWorker},
    render::RenderWorkerPool,
};
use geo::Geometry;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) render_worker_pool: Arc<RenderWorkerPool>,
    pub(crate) export_state: Arc<ExportState>,
    pub(crate) tile_cache_base_path: Arc<Option<PathBuf>>,
    pub(crate) tile_worker: Option<TileProcessingWorker>,
    pub(crate) serve_cached: bool,
    pub(crate) max_zoom: u8,
    pub(crate) limits_geometry: Arc<Option<Geometry>>,
    pub(crate) allowed_scales: Arc<Vec<f64>>,
}
