mod ctx_ext;
mod default;
mod feature_lines;
mod landcovers;
mod mapping;
mod pois;
mod roads;

use crate::render::layers::Category;
use crate::render::{ImageFormat, LegendValue, RenderLayer, RenderRequest};
use geo::{Coord, LineString, Polygon, Rect};
use indexmap::IndexMap;
use serde::Deserialize;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::f64;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::OnceLock;

#[derive(Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum LegendMode {
    Normal,
    Taginfo,
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
    pub zoom: u8,
}

pub struct LegendItemBuilder<'a> {
    pub id: &'a str,
    pub category: Category,
    pub tags: Vec<IndexMap<&'a str, &'a str>>,
    pub zoom: u8,
    pub data: LegendItemData,
    pub for_taginfo: bool,
}

impl<'a> LegendItem<'a> {
    pub fn builder(
        id: &'a str,
        category: Category,
        zoom: u8,
        for_taginfo: bool,
    ) -> LegendItemBuilder<'a> {
        LegendItemBuilder {
            id,
            category,
            tags: vec![],
            zoom,
            data: HashMap::new(),
            for_taginfo,
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
            zoom: self.zoom,
        }
    }

    fn add_tag_set(
        self,
        cb: impl FnOnce(TagsSetBuilder<'a>) -> TagsSetBuilder<'a>,
    ) -> LegendItemBuilder<'a> {
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

    fn add_landcover(self, typ: &'static str) -> Self {
        if self.for_taginfo {
            self
        } else {
            self.add_feature("landcovers", |b| {
                b.with("type", typ).with("name", "").with_polygon(true)
            })
        }
    }
}

pub struct TagsSetBuilder<'a> {
    parent: LegendItemBuilder<'a>,
}

impl<'a> TagsSetBuilder<'a> {
    fn add_tags(mut self, cb: impl FnOnce(TagsBuilder) -> TagsBuilder) -> TagsSetBuilder<'a> {
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

pub(crate) fn set_mapping_path(path: PathBuf) {
    if MAPPING_PATH.set(path).is_err() {
        panic!("mapping path already set");
    }
}

pub fn mapping_path() -> &'static PathBuf {
    MAPPING_PATH
        .get()
        .expect("mapping path must be set before legend use")
}

static LEGEND_ITEMS: LazyLock<Vec<LegendItem>> =
    LazyLock::new(|| default::build_legend_items(false));

static LEGEND_ITEMS_FOR_TAGINFO: LazyLock<Vec<LegendItem>> =
    LazyLock::new(|| default::build_legend_items(true));

pub fn legend_metadata() -> Vec<LegendMeta<'static>> {
    LEGEND_ITEMS.iter().map(|item| item.meta.clone()).collect()
}

pub fn legend_render_request(id: &str, scale: f64, mode: LegendMode) -> Option<RenderRequest> {
    let items = match mode {
        LegendMode::Normal => &LEGEND_ITEMS,
        LegendMode::Taginfo => &LEGEND_ITEMS_FOR_TAGINFO,
    };

    let (legend_item_data, zoom) = items
        .iter()
        .find(|item| item.meta.id == id)
        .map(|item| (item.data.clone(), item.zoom))?;

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

    Some(render_request)
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
    6378137.0 * 2.0 * f64::consts::PI / (256.0 * (zoom as f64).exp2())
}
