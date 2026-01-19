use crate::app::{export::ExportState, render_worker_pool::RenderWorkerPool};
use geo::Geometry;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) worker_pool: Arc<RenderWorkerPool>,
    pub(crate) export_state: Arc<ExportState>,
    pub(crate) tile_base_path: Arc<Option<PathBuf>>,
    pub(crate) index_zoom: u32,
    pub(crate) max_zoom: u32,
    pub(crate) limits_geometry: Arc<Option<Geometry>>,
    pub(crate) allowed_scales: Arc<Vec<f64>>,
}

impl AppState {
    pub fn new(
        worker_pool: RenderWorkerPool,
        tile_base_path: Option<PathBuf>,
        index_zoom: u32,
        max_zoom: u32,
        limits_geometry: Option<Geometry>,
        allowed_scales: Vec<f64>,
    ) -> Self {
        Self {
            worker_pool: Arc::new(worker_pool),
            export_state: Arc::new(ExportState::new()),
            tile_base_path: Arc::new(tile_base_path),
            index_zoom,
            max_zoom,
            limits_geometry: Arc::new(limits_geometry),
            allowed_scales: Arc::new(allowed_scales),
        }
    }
}
