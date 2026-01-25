use crate::render::{image_format::ImageFormat, layers::routes::RouteTypes};
use geo::Rect;
use geojson::Feature;

#[derive(Debug, Clone)]
pub struct RenderRequest {
    pub bbox: Rect<f64>,
    pub zoom: u32,
    pub scale: f64,
    pub format: ImageFormat,
    pub shading: bool,
    pub contours: bool,
    pub route_types: RouteTypes,
    pub featues: Option<Vec<Feature>>,
}

impl RenderRequest {
    pub const fn new(bbox: Rect<f64>, zoom: u32, scale: f64, format: ImageFormat) -> Self {
        Self {
            bbox,
            zoom,
            scale,
            format,
            shading: true,
            contours: true,
            route_types: RouteTypes::all(),
            featues: None,
        }
    }
}
