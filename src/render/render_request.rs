use crate::render::{image_format::ImageFormat, legend::LegendItemData};
use clap::ValueEnum;
use enumset::EnumSetType;
use geo::Geometry;
use geo::Rect;
use geojson::Feature;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Hash, ValueEnum, EnumSetType)]
pub enum RenderLayer {
    Shading,
    Contours,
    Sea,
    Geonames,
    CountryNames,
    CountryBorders,
    RoutesHiking,
    RoutesHikingKst,
    RoutesHorse,
    RoutesBicycle,
    RoutesSki,
}

#[derive(Debug, Clone)]
pub struct RenderRequest {
    pub bbox: Rect<f64>,
    pub zoom: u8,
    pub scale: f64,
    pub format: ImageFormat,
    pub to_render: HashSet<RenderLayer>,
    pub coverage_geometry: Option<Arc<Geometry>>,
    pub featues: Option<Vec<Feature>>,
    pub legend: Option<LegendItemData>,
}

impl RenderRequest {
    pub fn new(
        bbox: Rect<f64>,
        zoom: u8,
        scale: f64,
        format: ImageFormat,
        to_render: HashSet<RenderLayer>,
        coverage_geometry: Option<Arc<Geometry>>,
    ) -> Self {
        Self {
            bbox,
            zoom,
            scale,
            format,
            to_render,
            coverage_geometry,
            featues: None,
            legend: None,
        }
    }
}
