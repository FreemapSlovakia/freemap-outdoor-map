use crate::render::{legend::LegendItemData, projectable::TileProjector, size::Size};
use geo::Rect;
use tokio_postgres::types::ToSql;

pub struct SqlParams {
    params: Vec<Box<dyn ToSql + Sync>>,
}

// SAFETY: all values are inserted via `push` which requires `T: Send`,
// so every stored value is guaranteed to be Send.
unsafe impl Send for SqlParams {}

impl SqlParams {
    pub fn push<T>(mut self, value: T) -> Self
    where
        T: ToSql + Sync + Send + 'static,
    {
        self.params.push(Box::new(value));

        self
    }

    pub fn as_params(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params.iter().map(|param| param.as_ref()).collect()
    }
}

pub struct Ctx {
    pub bbox: Rect<f64>,
    pub size: Size<u32>,
    pub zoom: u8,
    pub tile_projector: TileProjector,
    pub scale: f64,
    pub legend: Option<LegendItemData>,
}

impl Ctx {
    pub fn meters_per_pixel(&self) -> f64 {
        self.bbox.width() / self.size.width as f64
    }

    pub fn bbox_query_params(&self, buffer_from_param: Option<f64>) -> SqlParams {
        let min = self.bbox.min();
        let max = self.bbox.max();

        let mut params: Vec<Box<dyn ToSql + Sync>> = vec![
            Box::new(min.x),
            Box::new(min.y),
            Box::new(max.x),
            Box::new(max.y),
        ];

        if let Some(buffer_from_param) = buffer_from_param {
            params.push(Box::new(self.meters_per_pixel() * buffer_from_param));
        }

        SqlParams { params }
    }

    pub(crate) fn hint(&self, x: f64) -> f64 {
        (x * self.scale).round() / self.scale
    }
}
