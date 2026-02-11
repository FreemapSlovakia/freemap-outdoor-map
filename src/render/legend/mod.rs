mod ctx_ext;
mod mapping;

use crate::render::layers::{Category, PAINT_DEFS, POI_ORDER, POIS};
use crate::render::{ImageFormat, LegendValue, RenderRequest};
use geo::{Coord, LineString, Point, Polygon, Rect};
use indexmap::IndexMap;
use mapping::{MappingKind, collect_mapping_entries};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::OnceLock;

#[derive(Clone, Serialize)]
pub struct LegendMeta<'a> {
    pub id: &'a str,
    pub category: Category,
    pub tags: Vec<IndexMap<&'a str, &'a str>>,
}

struct LegendItem<'a> {
    meta: LegendMeta<'a>,
    zoom: u8,
    data: LegendItemData,
}

impl<'a> LegendItem<'a> {
    fn new(
        id: &'static str,
        category: Category,
        tags: impl Into<Vec<IndexMap<&'static str, &'static str>>>,
        data: LegendItemData,
        zoom: u8,
    ) -> Self {
        Self {
            meta: LegendMeta {
                id,
                category,
                tags: tags.into(),
            },
            data,
            zoom,
        }
    }
}

static MAPPING_PATH: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_mapping_path(path: PathBuf) {
    if MAPPING_PATH.set(path).is_err() {
        panic!("mapping path already set");
    }
}

static LEGEND_ITEMS: LazyLock<Vec<LegendItem>> = LazyLock::new(|| {
    let mapping_path = MAPPING_PATH
        .get()
        .expect("mapping path must be set before legend use");

    let mapping_root: mapping::MappingRoot = {
        let mapping_file = std::fs::File::open(mapping_path).expect("read mapping.yaml");

        serde_saphyr::from_reader(BufReader::new(mapping_file)).expect("parse mapping.yaml")
    };

    let mut poi_tags: HashMap<&'static str, Vec<(&'static str, &'static str)>> = HashMap::new();
    let mut feature_alias_values: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    let mut feature_alias_catchall: HashSet<&'static str> = HashSet::new();

    let mut landcover_tags = HashMap::<&'static str, &'static str>::new();

    if let Some(pois) = mapping_root.tables.get("pois")
        && let Some(columns) = &pois.columns
    {
        for column in columns {
            if column.column_type != "mapping_value" {
                continue;
            }

            let Some(aliases) = &column.aliases else {
                continue;
            };

            for (key, values) in aliases {
                let key = leak_str(key);

                for (value, alias) in values {
                    let value = leak_str(value);
                    let alias = leak_str(alias);

                    if value == "__any__" {
                        feature_alias_catchall.insert(key);
                        poi_tags.entry(alias).or_default().push((key, "yes")); // "*"
                        continue;
                    }

                    feature_alias_values.entry(key).or_default().insert(value);

                    poi_tags.entry(alias).or_default().push((key, value));
                }
            }
        }
    }

    for entry in collect_mapping_entries(&mapping_root).into_iter() {
        if entry.table == "pois" || entry.table == "sports" {
            if feature_alias_catchall.contains(entry.key.as_str())
                || feature_alias_values
                    .get(entry.key.as_str())
                    .is_some_and(|values| values.contains(entry.value.as_str()))
            {
                continue;
            }

            let value = leak_str(&entry.value);
            let key = leak_str(&entry.key);

            poi_tags.entry(value).or_default().push((key, value));
        } else if entry.table == "landcovers"
            && matches!(
                entry.kind,
                MappingKind::TableMapping | MappingKind::TableMappingNested
            )
        {
            landcover_tags.insert(leak_str(&entry.value), leak_str(&entry.key));
        }
    }

    let mut poi_groups: IndexMap<
        &'static str,
        (
            Category,
            Vec<IndexMap<&'static str, &'static str>>,
            &'static str,
        ),
    > = IndexMap::new();

    for typ in POI_ORDER.iter() {
        let typ = *typ;

        if typ == "guidepost_noname" || typ.starts_with("peak") && typ.len() == 5 {
            continue;
        }

        let Some(defs) = POIS.get(typ) else {
            continue;
        };

        let Some(def) = defs.iter().find(|def| def.is_active_at(19)) else {
            continue;
        };

        let visual_key = def.icon_key(typ);

        let entry = poi_groups
            .entry(visual_key)
            .or_insert_with(|| (def.category, Vec::new(), typ));

        entry.1.push(build_poi_tags(typ, &poi_tags));
    }

    let poi_items = poi_groups
        .into_iter()
        .map(|(visual_key, (category, tags, repr_typ))| {
            LegendItem::new(
                format!("poi_{}", visual_key).leak(),
                category,
                tags,
                build_poi_data(&repr_typ, 19),
                19,
            )
        });

    let landcover_items = PAINT_DEFS.iter().map(|(types, _paints)| {
        let mut tags = Vec::with_capacity(types.len());

        for typ in *types {
            tags.push(build_landcover_tags(typ, &landcover_tags));
        }

        let id_typ = types[0];

        LegendItem::new(
            format!("landcover_{}", id_typ).leak(),
            Category::Landcover,
            tags,
            build_landcover_data(id_typ, 19),
            19,
        )
    });

    let other = vec![
        LegendItem::new(
            "line_tree_row",
            Category::NaturalPoi,
            [[("natural", "tree_row")].into()],
            build_line_data("tree_row", 17),
            17,
        ),
        LegendItem::new(
            "line_weir",
            Category::Water,
            [[("waterway", "weir")].into()],
            build_line_data("weir", 17),
            17,
        ),
        LegendItem::new(
            "line_dam",
            Category::Water,
            [[("waterway", "dam")].into()],
            build_line_data("dam", 17),
            17,
        ),
    ];

    let roads = (&[
        &["motorway", "trunk"] as &[&str],
        &["primary", "motorway_link", "trunk_link"],
        &["secondary", "primary_link", ""],
        &["tertiary", "tertiary_link", "secondary_link"],
        &["residential", "unclassified", "living_street", "road"],
        &["service"],
        &["footway", "pedestrian"],
        &["platform"],
        &["steps"],
        &["cycleway"],
        &["path"],
        &["piste"],
        &["bridleway"],
        &["via_ferrata"],
        &["track"],
    ])
        .into_iter()
        .enumerate()
        .map(|(i, types)| {
            LegendItem::new(
                format!("road_{}", types[0]).leak(),
                Category::Communications,
                types
                    .iter()
                    .map(|typ| IndexMap::from([("highway", *typ)]))
                    .collect::<Vec<_>>(),
                with_landcover(if i < 10 { "residential" } else { "wood" }, 17)
                    .with_feature(
                        "roads",
                        road_builder(types[0], 17).with("class", "highway").build(),
                    )
                    .build(),
                17,
            )
        });

    let tracks = (1..=5).map(|grade| {
        let grade: &str = format!("grade{grade}").leak();

        LegendItem::new(
            format!("road_track_{grade}").leak(),
            Category::Communications,
            vec![[("highway", "track"), ("tracktype", grade)].into()],
            with_landcover("wood", 17)
                .with_feature(
                    "roads",
                    road_builder("track", 17)
                        .with("class", "highway")
                        .with("tracktype", grade)
                        .build(),
                )
                .build(),
            17,
        )
    });

    let visibilities = ["excellent", "good", "intermediate", "bad", "horrible", "no"]
        .into_iter()
        .enumerate()
        .map(|(i, visibility)| {
            LegendItem::new(
                format!("road_visibility_{visibility}").leak(),
                Category::Communications,
                vec![[("highway", "path"), ("trail_visibility", visibility)].into()],
                with_landcover("wood", 17)
                    .with_feature(
                        "roads",
                        road_builder("path", 17)
                            .with("class", "highway")
                            .with("trail_visibility", i as i32)
                            .build(),
                    )
                    .build(),
                17,
            )
        });

    poi_items
        .chain(landcover_items)
        .chain(roads)
        .chain(tracks)
        .chain(visibilities)
        .chain(other)
        .collect()
});

pub fn legend_metadata() -> Vec<LegendMeta<'static>> {
    LEGEND_ITEMS.iter().map(|item| item.meta.clone()).collect()
}

// layer -> "tags"
pub type LegendItemData = HashMap<String, Vec<HashMap<String, LegendValue>>>;

type LegendFeatureData = HashMap<String, LegendValue>;

pub fn legend_render_request(id: &str, scale: f64) -> Option<RenderRequest> {
    let (legend_item_data, zoom) = LEGEND_ITEMS
        .iter()
        .find(|item| item.meta.id == id)
        .map(|item| (item.data.clone(), item.zoom))?;

    let zoom_factor = (20f64 - zoom as f64).exp2();

    let bbox = Rect::new(
        Coord {
            x: -8.0 * zoom_factor,
            y: -4.0 * zoom_factor,
        },
        Coord {
            x: 8.0 * zoom_factor,
            y: 4.0 * zoom_factor,
        },
    );

    let mut render_request = RenderRequest::new(bbox, zoom, scale, ImageFormat::Jpeg);

    render_request.legend = Some(legend_item_data);

    Some(render_request)
}

fn build_poi_tags(
    typ: &'static str,
    poi_tags: &HashMap<&'static str, Vec<(&'static str, &'static str)>>,
) -> IndexMap<&'static str, &'static str> {
    let mut tags = vec![];

    if matches!(
        typ,
        "convenience"
            | "confectionery"
            | "pastry"
            | "bicycle"
            | "supermarket"
            | "greengrocer"
            | "farm"
    ) {
        tags.push(("shop", typ));
    } else if matches!(
        typ,
        "shopping_cart"
            | "lean_to"
            | "public_transport"
            | "picnic_shelter"
            | "basic_hut"
            | "weather_shelter"
    ) {
        tags.push(("amenity", "shelter"));
        tags.push(("shelter_type", typ));
    } else {
        let mut override_key = None;

        match typ {
            s if typ.starts_with("tower_") || typ.starts_with("mast_") => {
                let (a, b) = typ.split_once("_").unwrap();
                tags.push(("man_made", a));
                tags.push(("tower:type", b));
            }
            "tree_protected" => {
                override_key = Some("tree");
                tags.push(("protected", "yes"));
            }
            "generator_wind" => {
                tags.push(("power", "generator"));
                tags.push(("generator:source", "wind")); // OR method = 'wind_turbine'
            }
            "church" | "chapel" | "synagogue" | "mosque" | "cathedral" => {
                tags.push(("building", typ));
            }
            "disused_mine" | "disused_adit" | "disused_mineshaft" => {
                override_key = Some(&typ[8..]);
                tags.push(("disused", "yes"));
            }
            _ => {}
        };

        if let Some(pairs) = poi_tags.get(override_key.unwrap_or(typ)) {
            for (key, value) in pairs {
                if *key == "information" {
                    tags.push(("tourism", key));
                }

                tags.push((key, value));
            }
        }
    }

    build_tags_map(tags)
}

fn build_landcover_tags(
    typ: &'static str,
    landcover_tags: &HashMap<&'static str, &'static str>,
) -> IndexMap<&'static str, &'static str> {
    let mut tags = vec![];

    if let Some(value) = landcover_tags.get(typ) {
        tags.push((*value, typ));
    }

    if matches!(
        typ,
        "bog" | "reedbed" | "marsh" | "swamp" | "wet_meadow" | "mangrove" | "fen"
    ) {
        tags.push(("natural", "wetland"));
        tags.push(("wetland", typ));
    }

    build_tags_map(tags)
}

fn build_tags_map(tags: Vec<(&'static str, &'static str)>) -> IndexMap<&'static str, &'static str> {
    let mut t = IndexMap::with_capacity(tags.len());

    for (k, v) in tags {
        t.insert(k, v);
    }

    t
}

fn build_poi_data(typ: &'static str, zoom: u8) -> LegendItemData {
    let factor = (19.0 - zoom as f64).exp2();

    with_landcover("wood", zoom)
        .with_feature(
            "pois",
            legend_feature_data_builder()
                .with("type", typ)
                .with("name", "Abc")
                .with("extra", HashMap::<String, Option<String>>::new())
                .with("geometry", Point::new(0.0, factor * -2.0))
                .build(),
        )
        .build()
}

fn build_line_data(typ: &'static str, zoom: u8) -> LegendItemData {
    with_landcover("wood", zoom)
        .with_feature(
            "feature_lines",
            legend_feature_data_builder()
                .with("type", typ)
                .with("name", "Abc")
                .with("extra", HashMap::<String, Option<String>>::new())
                .with_line_string(zoom)
                .build(),
        )
        .build()
}

fn build_landcover_data(typ: &'static str, zoom: u8) -> LegendItemData {
    legend_item_data_builder()
        .with_feature(
            "landcovers",
            legend_feature_data_builder()
                .with("type", typ)
                .with("name", "Abc")
                .with("geometry", polygon(true, zoom))
                .build(),
        )
        .build()
}

#[derive(Default)]
struct LegendFeatureDataBuilder(LegendFeatureData);

impl LegendFeatureDataBuilder {
    fn with(mut self, key: impl Into<String>, value: impl Into<LegendValue>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    fn with_line_string(self, zoom: u8) -> Self {
        let factor = (17.0 - zoom as f64).exp2();

        self.with(
            "geometry",
            LineString::new(vec![
                Coord {
                    x: -80.0 * factor,
                    y: -20.0 * factor,
                },
                Coord {
                    x: 80.0 * factor,
                    y: 20.0 * factor,
                },
            ]),
        )
    }

    fn build(self) -> LegendFeatureData {
        self.0
    }
}

#[derive(Default)]
struct LegendItemDataBuilder(LegendItemData);

impl LegendItemDataBuilder {
    fn with_layer(mut self, layer: impl Into<String>, features: Vec<LegendFeatureData>) -> Self {
        self.0.insert(layer.into(), features);
        self
    }

    fn with_feature(self, layer: impl Into<String>, feature: LegendFeatureData) -> Self {
        self.with_layer(layer, vec![feature])
    }

    fn build(self) -> LegendItemData {
        self.0
    }
}

fn legend_feature_data_builder() -> LegendFeatureDataBuilder {
    LegendFeatureDataBuilder::default()
}

fn legend_item_data_builder() -> LegendItemDataBuilder {
    LegendItemDataBuilder::default()
}

fn with_landcover(typ: &'static str, zoom: u8) -> LegendItemDataBuilder {
    legend_item_data_builder().with_feature(
        "landcovers",
        legend_feature_data_builder()
            .with("type", typ)
            .with("name", "")
            .with("geometry", polygon(true, zoom))
            .build(),
    )
}

fn road_builder(typ: &'static str, zoom: u8) -> LegendFeatureDataBuilder {
    legend_feature_data_builder()
        .with("type", typ)
        .with("name", "Abc")
        .with("tracktype", "")
        .with("class", "")
        .with("service", "")
        .with("bridge", 0i16)
        .with("tunnel", 0i16)
        .with("oneway", 0i16)
        .with("bicycle", "")
        .with("foot", "")
        .with("trail_visibility", 0)
        .with_line_string(zoom)
}

fn polygon(skew: bool, zoom: u8) -> Polygon {
    let factor = (19.0 - zoom as f64).exp2();

    let ssx = if skew { 2.0 } else { 0.0 };
    let ssy = if skew { 1.0 } else { 0.0 };

    let xx = 12.0;
    let yy = 6.0;

    Polygon::new(
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
        ]),
        vec![],
    )
}

fn leak_str(value: &str) -> &'static str {
    value.to_string().leak()
}
