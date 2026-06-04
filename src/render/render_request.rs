use crate::render::{colors::Color, image_format::ImageFormat, legend::LegendItemData};
use clap::ValueEnum;
use colorsys::RgbRatio;
use cosmic_text::Weight;
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

/// Glow halo drawn behind the custom features.
#[derive(Debug, Clone)]
pub struct Glow {
    /// Halo color; its alpha is the opacity at which the whole glow layer is
    /// composited.
    pub color: RgbRatio,
    /// Width (in tile/CSS pixels) the halo extends on each side of a line /
    /// polygon edge or marker outline.
    pub width: f64,
}

/// Optional styling overrides for the custom layer's text labels (the feature
/// `title`s). Each `None` keeps the built-in default, which differs per label
/// kind (point labels are blue and bold, line/polygon labels follow the text
/// defaults), so the overrides are applied field-by-field rather than as a
/// whole style.
#[derive(Debug, Clone, Copy, Default)]
pub struct LabelStyle {
    /// Text fill color. `None` keeps the per-kind default.
    pub color: Option<Color>,
    /// Font weight. `None` keeps the per-kind default.
    pub weight: Option<Weight>,
    /// Font size in tile/CSS pixels. `None` keeps the default (`15.0`).
    pub size: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct CustomLayer {
    pub features: Vec<Feature>,
    pub order: CustomLayerOrder,
    /// Rendered width (in tile/CSS pixels) of a drawing-point marker.
    pub marker_width: f64,
    /// Optional glow halo. `None` disables the glow entirely.
    pub glow_color: Option<Glow>,
    /// Styling overrides for feature `title` labels.
    pub label_style: LabelStyle,
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
