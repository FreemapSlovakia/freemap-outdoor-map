use crate::{
    app::{
        server::{export_route::ExportState, licenses_route::LicenseCatalog},
        tile_processing_worker::TileProcessingWorker,
    },
    render::{ImageFormat, Layers, RenderLayer, RenderWorkerPool},
};
use geo::Geometry;
use std::{collections::HashSet, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct TileVariantState {
    pub(crate) url_path: String,
    pub(crate) tile_cache_base_path: Option<PathBuf>,
    pub(crate) coverage_geometry: Option<Arc<Geometry>>,
    pub(crate) layers: Layers,
    pub(crate) format: ImageFormat,
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) render_worker_pool: Arc<RenderWorkerPool>,
    pub(crate) export_state: Arc<ExportState>,
    pub(crate) licenses: Arc<LicenseCatalog>,
    pub(crate) tile_variants: Arc<Vec<TileVariantState>>,
    pub(crate) default_render: HashSet<RenderLayer>,
    pub(crate) tile_worker: Option<TileProcessingWorker>,
    pub(crate) serve_cached: bool,
    pub(crate) min_zoom: u8,
    pub(crate) max_zoom: u8,
    pub(crate) allowed_scales: Vec<f64>,
}

impl AppState {
    /// The variant serving `url_path`, as the tile routes spell it.
    pub(crate) fn variant_by_path(&self, url_path: &str) -> Option<&TileVariantState> {
        self.tile_variants
            .iter()
            .find(|variant| variant.url_path == url_path)
    }
}

#[derive(Clone)]
pub struct TileRouteState {
    pub(crate) app_state: AppState,
    pub(crate) variant_index: usize,
}
