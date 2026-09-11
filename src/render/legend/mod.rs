mod default;
mod feature_lines;
mod landcovers;
mod mapping;
mod pois;
mod roads;

#[cfg(test)]
mod zoom_range_test;

use crate::render::layers::Category;
use crate::render::{ImageFormat, LegendValue, RenderLayer, RenderRequest};
use geo::{Coord, LineString, Polygon, Rect};
use indexmap::IndexMap;
use serde::Deserialize;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::f64;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::OnceLock;

#[derive(Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum LegendMode {
    Normal,
    Taginfo,
}

/// Highest zoom a legend item may be built for. Legend renders are not tiles, so this is
/// independent of `--max-zoom`; it only bounds the per-zoom item cache.
pub const MAX_LEGEND_ZOOM: u8 = 20;

/// Inputs shared by every legend item builder.
#[derive(Clone, Copy)]
pub struct BuildOpts {
    /// Render every item at this zoom instead of at its own preferred zoom.
    pub zoom: Option<u8>,
    pub for_taginfo: bool,
}

#[derive(Clone, Serialize)]
pub struct LegendMeta<'a> {
    pub id: &'a str,
    pub category: Category,
    pub tags: Vec<IndexMap<&'a str, &'a str>>,
}

pub struct LegendItem<'a> {
    pub meta: LegendMeta<'a>,
    pub data: LegendItemData,
    /// Landcover drawn underneath so the symbol is legible. Kept apart from `data` so it can
    /// be rendered on its own as the "nothing to see here" baseline (see the zoom range test).
    pub background: LegendItemData,
    pub zoom: u8,
    /// Zooms at which the map actually shows this feature. Mirrors the gating in
    /// `layers::pipeline` and the layer render fns; kept honest by `zoom_range_test`.
    pub zooms: RangeInclusive<u8>,
    /// Whether `zoom_range_test` can verify the lower end of `zooms` by rendering.
    #[cfg_attr(not(test), allow(dead_code))]
    pub probe_lower_edge: bool,
}

pub struct LegendItemBuilder<'a> {
    pub id: &'a str,
    pub category: Category,
    pub tags: Vec<IndexMap<&'a str, &'a str>>,
    pub zoom: u8,
    pub zooms: RangeInclusive<u8>,
    pub probe_lower_edge: bool,
    pub data: LegendItemData,
    pub background: LegendItemData,
    pub for_taginfo: bool,
}

impl LegendItem<'_> {
    /// The item's own features drawn on top of its background landcover.
    pub fn render_data(&self) -> LegendItemData {
        let mut data = self.background.clone();

        for (layer, features) in &self.data {
            data.entry(layer.clone())
                .or_default()
                .extend(features.iter().cloned());
        }

        data
    }
}

impl<'a> LegendItem<'a> {
    /// `preferred_zoom` is the zoom the item is drawn at when the request does not ask for
    /// a specific one; [`BuildOpts::zoom`] overrides it.
    pub fn builder(
        id: &'a str,
        category: Category,
        preferred_zoom: u8,
        opts: BuildOpts,
    ) -> LegendItemBuilder<'a> {
        LegendItemBuilder {
            id,
            category,
            tags: vec![],
            zoom: opts.zoom.unwrap_or(preferred_zoom),
            zooms: 0..=MAX_LEGEND_ZOOM,
            probe_lower_edge: true,
            data: HashMap::new(),
            background: HashMap::new(),
            for_taginfo: opts.for_taginfo,
        }
    }
}

impl<'a> LegendItemBuilder<'a> {
    pub fn build(self) -> LegendItem<'a> {
        LegendItem {
            meta: LegendMeta {
                id: self.id,
                category: self.category,
                tags: self.tags,
            },
            data: self.data,
            background: self.background,
            zoom: self.zoom,
            zooms: self.zooms,
            probe_lower_edge: self.probe_lower_edge,
        }
    }

    /// Lowest zoom at which the map draws this feature.
    const fn min_zoom(self, min: u8) -> Self {
        let max = *self.zooms.end();

        self.zoom_range(min, max)
    }

    /// Like [`Self::min_zoom`], for a lower edge the zoom range test cannot probe: the sample
    /// still ends up on a legend render below `min`, either because the detail the entry is
    /// about (a bridge casing, a oneway arrow) is missing from an otherwise identical drawing,
    /// or because the map drops the feature in the layer's SQL, which legend renders skip.
    const fn min_zoom_unprobeable(mut self, min: u8) -> Self {
        self.probe_lower_edge = false;

        self.min_zoom(min)
    }

    /// Zoom span at which the map draws this feature, both ends inclusive.
    const fn zoom_range(self, min: u8, max: u8) -> Self {
        self.zoom_range_of(min..=max)
    }

    const fn zoom_range_of(mut self, zooms: RangeInclusive<u8>) -> Self {
        self.zooms = zooms;

        self
    }

    fn add_tag_set(self, cb: impl FnOnce(TagsSetBuilder<'a>) -> TagsSetBuilder<'a>) -> Self {
        let tsb = cb(TagsSetBuilder { parent: self });

        tsb.parent
    }

    fn add_feature(
        mut self,
        layer: impl Into<String>,
        cb: impl FnOnce(PropsBuilder) -> PropsBuilder,
    ) -> Self {
        let props_builder = cb(PropsBuilder {
            zoom: self.zoom,
            for_taginfo: self.for_taginfo,
            props: HashMap::new(),
        });

        self.data
            .entry(layer.into())
            .or_default()
            .push(props_builder.props);

        self
    }

    fn add_landcover(mut self, typ: &'static str) -> Self {
        if self.for_taginfo {
            return self;
        }

        let props_builder = PropsBuilder {
            zoom: self.zoom,
            for_taginfo: self.for_taginfo,
            props: HashMap::new(),
        }
        .with("type", typ)
        .with("name", "")
        .with_polygon(true);

        self.background
            .entry("landcovers".to_owned())
            .or_default()
            .push(props_builder.props);

        self
    }
}

pub struct TagsSetBuilder<'a> {
    parent: LegendItemBuilder<'a>,
}

impl TagsSetBuilder<'_> {
    fn add_tags(mut self, cb: impl FnOnce(TagsBuilder) -> TagsBuilder) -> Self {
        let tb = cb(TagsBuilder {
            tags: IndexMap::new(),
        });

        self.parent.tags.push(tb.tags);

        self
    }
}

pub struct TagsBuilder<'a> {
    tags: IndexMap<&'a str, &'a str>,
}

impl<'a> TagsBuilder<'a> {
    pub fn add(mut self, key: &'a str, value: &'a str) -> Self {
        self.tags.insert(key, value);
        self
    }
}

pub struct PropsBuilder {
    zoom: u8,
    for_taginfo: bool,
    props: HashMap<String, LegendValue>,
}

impl PropsBuilder {
    pub fn with(mut self, key: impl Into<String>, value: impl Into<LegendValue>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    pub fn with_name(self) -> Self {
        let for_taginfo = self.for_taginfo;

        self.with("name", if for_taginfo { "" } else { "Abc" })
    }
}

static MAPPING_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn set_mapping_path(path: PathBuf) {
    assert!(MAPPING_PATH.set(path).is_ok(), "mapping path already set");
}

/// [`set_mapping_path`] for tests: the whole suite is one process, so every test that needs
/// the mapping has to be able to ask for it without racing to be the one that sets it.
#[cfg(test)]
fn set_mapping_path_for_test() {
    let _ = MAPPING_PATH.set("mapping.yaml".into());
}

pub fn mapping_path() -> &'static PathBuf {
    MAPPING_PATH
        .get()
        .expect("mapping path must be set before legend use")
}

/// One slot per zoom, plus a trailing slot for "each item at its own preferred zoom".
const SLOT_COUNT: usize = MAX_LEGEND_ZOOM as usize + 2;

const PREFERRED_ZOOM_SLOT: usize = MAX_LEGEND_ZOOM as usize + 1;

type ZoomSlots = [OnceLock<Vec<LegendItem<'static>>>; SLOT_COUNT];

/// Items are built lazily per (zoom, taginfo) combination and kept forever; building is
/// cheap once the mapping file is parsed, and the items borrow leaked `'static` strs.
static LEGEND_ITEMS: LazyLock<[ZoomSlots; 2]> =
    LazyLock::new(|| std::array::from_fn(|_| std::array::from_fn(|_| OnceLock::new())));

/// A zoom above [`MAX_LEGEND_ZOOM`] is clamped rather than rejected — callers pass through
/// user input, and an out-of-range zoom has no sensible rendering anyway.
fn clamp_zoom(zoom: u8) -> u8 {
    zoom.min(MAX_LEGEND_ZOOM)
}

fn legend_items(zoom: Option<u8>, for_taginfo: bool) -> &'static [LegendItem<'static>] {
    let zoom = zoom.map(clamp_zoom);

    let slot = zoom.map_or(PREFERRED_ZOOM_SLOT, usize::from);

    LEGEND_ITEMS[usize::from(for_taginfo)][slot]
        .get_or_init(|| default::build_legend_items(BuildOpts { zoom, for_taginfo }))
}

/// Metadata for every legend item, or — with `zoom` — only for those the map actually
/// draws at that zoom.
pub fn legend_metadata(zoom: Option<u8>) -> Vec<LegendMeta<'static>> {
    let zoom = zoom.map(clamp_zoom);

    // Zoom only decides which items are listed, never what their metadata says, so this reads
    // the items built at their preferred zooms instead of building a set of its own.
    legend_items(None, false)
        .iter()
        .filter(|item| zoom.is_none_or(|zoom| item.zooms.contains(&zoom)))
        .map(|item| item.meta.clone())
        .collect()
}

pub fn legend_render_request(
    id: &str,
    zoom: Option<u8>,
    scale: f64,
    mode: LegendMode,
) -> Option<RenderRequest> {
    let items = legend_items(zoom, mode == LegendMode::Taginfo);

    let item = items.iter().find(|item| item.meta.id == id)?;

    Some(render_request(item.render_data(), item.zoom, scale, mode))
}

fn render_request(
    legend_item_data: LegendItemData,
    zoom: u8,
    scale: f64,
    mode: LegendMode,
) -> RenderRequest {
    let bbox = match mode {
        LegendMode::Normal => {
            let zoom_factor = (20f64 - zoom as f64).exp2();

            Rect::new(
                Coord {
                    x: -8.0 * zoom_factor,
                    y: -3.5 * zoom_factor,
                },
                Coord {
                    x: 8.0 * zoom_factor,
                    y: 3.5 * zoom_factor,
                },
            )
        }
        LegendMode::Taginfo => {
            let px = 8.0 * to_px(zoom);

            Rect::new(Coord { x: -px, y: -px }, Coord { x: px, y: px })
        }
    };

    let mut render_request = RenderRequest::new(
        bbox,
        zoom,
        scale,
        match mode {
            LegendMode::Normal => ImageFormat::Png,
            LegendMode::Taginfo => ImageFormat::Svg,
        },
        HashSet::from([
            RenderLayer::CountryBorders,
            RenderLayer::RoutesBicycle,
            RenderLayer::RoutesHiking,
            RenderLayer::RoutesHorse,
            RenderLayer::RoutesSki,
        ]),
        None,
    );

    render_request.legend = Some(legend_item_data);

    render_request
}

impl PropsBuilder {
    pub fn with_line_string(self, reverse: bool) -> Self {
        let mut coords = if self.for_taginfo {
            let px = 10.0 * to_px(self.zoom);

            vec![Coord { x: px, y: 0.0 }, Coord { x: -px, y: 0.0 }]
        } else {
            let factor = (17.0 - self.zoom as f64).exp2();

            vec![
                Coord {
                    x: 80.0 * factor,
                    y: 20.0 * factor,
                },
                Coord {
                    x: -80.0 * factor,
                    y: -20.0 * factor,
                },
            ]
        };

        if reverse {
            coords.reverse();
        }

        self.with("geometry", LineString::new(coords))
    }

    pub fn with_polygon(self, skew: bool) -> Self {
        let zoom = self.zoom;
        let for_taginfo = self.for_taginfo;

        let px = 7.0 * to_px(zoom);

        self.with(
            "geometry",
            Polygon::new(
                if for_taginfo {
                    LineString::new(vec![
                        Coord { x: -px, y: -px },
                        Coord { x: -px, y: px },
                        Coord { x: px, y: px },
                        Coord { x: px, y: -px },
                        Coord { x: -px, y: -px },
                    ])
                } else {
                    let factor = (19.0 - zoom as f64).exp2();

                    let ssx = if skew { 2.0 } else { 0.0 };
                    let ssy = if skew { 1.0 } else { 0.0 };

                    let xx = 12.0;
                    let yy = 5.0;

                    LineString::new(vec![
                        Coord {
                            x: factor * -xx,
                            y: factor * (-yy - ssy),
                        },
                        Coord {
                            x: factor * (-xx - ssx),
                            y: factor * yy,
                        },
                        Coord {
                            x: factor * xx,
                            y: factor * (yy + ssy),
                        },
                        Coord {
                            x: factor * (xx + ssx),
                            y: factor * -yy,
                        },
                        Coord {
                            x: factor * -xx,
                            y: factor * (-yy - ssy),
                        },
                    ])
                },
                vec![],
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{legend_metadata, set_mapping_path_for_test};

    /// Every tag set on a legend item has to say something. The frontend turns each set into
    /// a name for the reader, so an empty one renders as a nameless row beside the icon -
    /// visible on the map's legend page, and silent everywhere else. `temple` did exactly
    /// that: it draws the church icon, so it joins the `poi_church` item, but it had no arm
    /// in `build_poi_tags` and nothing in mapping.yaml to fall through to.
    #[test]
    fn every_legend_item_names_its_tags() {
        set_mapping_path_for_test();

        let mut bad = vec![];

        for meta in legend_metadata(None) {
            if meta.tags.is_empty() {
                bad.push(format!("{} lists no tags at all", meta.id));
            }

            for (idx, tags) in meta.tags.iter().enumerate() {
                if tags.is_empty() {
                    bad.push(format!("{}: tag set {idx} is empty", meta.id));
                }
            }
        }

        assert!(
            bad.is_empty(),
            "legend items showing a nameless row:\n{}",
            bad.join("\n")
        );
    }
}

pub type LegendItemData = HashMap<String, Vec<LegendFeatureData>>; // layer -> prop_map[]
pub type LegendFeatureData = HashMap<String, LegendValue>;

pub fn build_tags_map(
    tags: Vec<(&'static str, &'static str)>,
) -> IndexMap<&'static str, &'static str> {
    let mut map = IndexMap::with_capacity(tags.len());

    for (k, v) in tags {
        map.insert(k, v);
    }

    map
}

pub fn leak_str(value: &str) -> &'static str {
    value.to_string().leak()
}

fn to_px(zoom: u8) -> f64 {
    6_378_137.0 * f64::consts::TAU / (256.0 * (zoom as f64).exp2())
}
