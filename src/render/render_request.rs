use crate::render::{colors::Color, image_format::ImageFormat, legend::LegendItemData};
use clap::ValueEnum;
use colorsys::RgbRatio;
use cosmic_text::Weight;
use enumset::EnumSetType;
use geo::Geometry;
use geo::Rect;
use geojson::Feature;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
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
    SacScale,
    Smoothness,
    MtbScale,
    Waymarking,
    Landcover,
    WaterAreas,
    Buildings,
    PierAreas,
    BridgeAreas,
    SolarPlants,
    Trees,
    Cutlines,
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
    pub attribution: Option<AttributionDecoration>,
    pub center_lat: f64,
}

/// What to draw the attribution line from. The datasets are not in here: only
/// the finished render knows which of them contributed a pixel, so the text is
/// composed at drawing time from the render's own codes.
#[derive(Debug, Clone)]
pub struct AttributionDecoration {
    /// Credits the renderer cannot know — whatever the exported features earn.
    /// Drawn after this map's own credit, in the order given.
    pub extra: Vec<String>,
    /// Dataset code to the titles crediting it, for every code there is.
    pub catalog: Arc<BTreeMap<String, Vec<String>>>,
    /// Titles the caller words itself, by code. Only `map` — this renderer's
    /// own credit — and OpenStreetMap's are honoured, because the catalog holds
    /// those in English alone. Every other title is a rights-holder's own
    /// string in every language, and one code can carry several of them, which
    /// a single replacement could not stand in for.
    pub overrides: HashMap<String, String>,
}

impl RenderLayer {
    /// Whether the map draws this of its own accord. A base layer is part of the
    /// map and goes only when a request takes it away; an extra is never drawn
    /// unless a request asks for it.
    ///
    /// The split is what lets [`Layers`] keep two lists that each mean one thing.
    pub const fn is_base(self) -> bool {
        matches!(
            self,
            Self::Sea
                | Self::Landcover
                | Self::WaterAreas
                | Self::Buildings
                | Self::PierAreas
                | Self::BridgeAreas
                | Self::SolarPlants
                | Self::Trees
                | Self::Cutlines
        )
    }
}

/// Which layers a request draws.
///
/// `add` names extras to draw and `omit` names base layers to drop — never the
/// other way round, which [`Layers::validate`] enforces. An overlay is
/// `base_map: false` plus the extras it wants; an overlay for something that
/// draws its own ground, an aerial image above all, is `base_map: true` with the
/// ground omitted.
///
/// Anything but `base_map: true` with an empty `omit` leaves part of the surface
/// unpainted, so it needs an alpha-capable format to be of any use.
#[derive(Debug, Clone)]
pub struct Layers {
    /// Whether the layers the map draws by itself are drawn at all.
    pub base_map: bool,
    /// Extras to draw. Only [`RenderLayer`]s that are not `is_base`.
    pub add: HashSet<RenderLayer>,
    /// Base layers to drop. Only [`RenderLayer`]s that are `is_base`.
    pub omit: HashSet<RenderLayer>,
}

impl Layers {
    /// The whole map, plus `add`.
    pub fn map(add: HashSet<RenderLayer>) -> Self {
        Self {
            base_map: true,
            add,
            omit: HashSet::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(layer) = self.add.iter().find(|layer| layer.is_base()) {
            return Err(format!(
                "{layer:?} is part of the map, so it belongs in the omit list, not the render list"
            ));
        }

        if let Some(layer) = self.omit.iter().find(|layer| !layer.is_base()) {
            return Err(format!(
                "{layer:?} is not part of the map, so omitting it does nothing - leave it out of the render list instead"
            ));
        }

        Ok(())
    }

    /// Whether `layer` is drawn.
    pub fn draws(&self, layer: RenderLayer) -> bool {
        if layer.is_base() {
            self.base_map && !self.omit.contains(&layer)
        } else {
            self.add.contains(&layer)
        }
    }

    /// Whether the map's own layers are drawn untouched — the only case that
    /// paints the whole surface.
    pub fn is_whole_map(&self) -> bool {
        self.base_map && self.omit.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct RenderRequest {
    pub bbox: Rect<f64>,
    pub zoom: u8,
    pub scale: f64,
    pub format: ImageFormat,
    pub layers: Layers,
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
        layers: Layers,
        coverage_geometry: Option<Arc<Geometry>>,
    ) -> Self {
        Self {
            bbox,
            zoom,
            scale,
            format,
            layers,
            coverage_geometry,
            custom_layer: None,
            legend: None,
            decorations: None,
        }
    }
}
