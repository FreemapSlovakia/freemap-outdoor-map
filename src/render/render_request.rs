use std::collections::HashSet;

use crate::render::{image_format::ImageFormat, legend::LegendItemData};
use clap::ValueEnum;
use geo::Rect;
use geojson::Feature;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, ValueEnum)]
pub enum RenderLayer {
    Shading,
    Contours,
    Sea,
    Geonames,
    CountryNames,
    CountryBorders,
    RoutesHiking,
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
    pub render: HashSet<RenderLayer>,
    pub featues: Option<Vec<Feature>>,
    pub legend: Option<LegendItemData>,
}

impl RenderRequest {
    pub fn new(
        bbox: Rect<f64>,
        zoom: u8,
        scale: f64,
        format: ImageFormat,
        render: HashSet<RenderLayer>,
    ) -> Self {
        Self {
            bbox,
            zoom,
            scale,
            format,
            render,
            featues: None,
            legend: None,
        }
    }
}
