use crate::render::{image_format::ImageFormat, legend::LegendItemData};
use clap::ValueEnum;
use enumset::EnumSetType;
use geo::Geometry;
use geo::Rect;
use geojson::Feature;
use serde::Deserialize;
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

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum CustomLayerOrder {
    Natural,
    Topmost,
}

#[derive(Debug, Clone)]
pub struct CustomLayer {
    pub features: Vec<Feature>,
    pub order: CustomLayerOrder,
}

/// Cartographic decorations drawn on top of the finished map (scale bar, north
/// arrow, attribution). All opt-in (a `None`/`false` field is omitted). The
/// north-arrow label is provided by the client for localization — "N" in
/// English but "S" (sever) in Slovak; the scale bar uses the universal SI unit
/// symbols (m/km) directly. `center_lat` is the bbox center latitude in degrees
/// (WGS84), used to correct the Web-Mercator scale for the scale bar.
#[derive(Debug, Clone)]
pub struct Decorations {
    pub scale_bar: bool,
    pub north_arrow: Option<String>,
    pub attribution: Option<String>,
    pub center_lat: f64,
}

#[derive(Debug, Clone)]
pub struct RenderRequest {
    pub bbox: Rect<f64>,
    pub zoom: u8,
    pub scale: f64,
    pub format: ImageFormat,
    pub to_render: HashSet<RenderLayer>,
    pub coverage_geometry: Option<Arc<Geometry>>,
    pub custom_layer: Option<CustomLayer>,
    pub legend: Option<LegendItemData>,
    pub decorations: Option<Decorations>,
}

impl RenderRequest {
    pub const fn new(
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
            custom_layer: None,
            legend: None,
            decorations: None,
        }
    }
}
