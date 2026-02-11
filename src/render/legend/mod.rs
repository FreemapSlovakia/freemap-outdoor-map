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
pub struct LegendMeta {
    pub id: String,
    pub category: Category,
    pub tags: Vec<IndexMap<String, String>>,
}

struct LegendItem {
    meta: LegendMeta,
    zoom: u8,
    data: LegendItemData,
}

impl LegendItem {
    fn new(
        id: String,
        category: Category,
        tags: Vec<IndexMap<String, String>>,
        data: LegendItemData,
        zoom: u8,
    ) -> Self {
        Self {
            meta: LegendMeta { id, category, tags },
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

    let mut poi_tags: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut feature_alias_values: HashMap<String, HashSet<String>> = HashMap::new();
    let mut feature_alias_catchall: HashSet<String> = HashSet::new();

    let mut landcover_tags = HashMap::new();

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
                for (value, alias) in values {
                    if value == "__any__" {
                        feature_alias_catchall.insert(key.to_string());
                        poi_tags
                            .entry(alias.to_string())
                            .or_default()
                            .push((key.to_string(), "*".to_string()));
                        continue;
                    }

                    feature_alias_values
                        .entry(key.to_string())
                        .or_default()
                        .insert(value.to_string());

                    poi_tags
                        .entry(alias.to_string())
                        .or_default()
                        .push((key.to_string(), value.to_string()));
                }
            }
        }
    }

    for entry in collect_mapping_entries(&mapping_root).into_iter() {
        if entry.table == "pois" || entry.table == "sports" {
            if feature_alias_catchall.contains(&entry.key)
                || feature_alias_values
                    .get(&entry.key)
                    .is_some_and(|values| values.contains(&entry.value))
            {
                continue;
            }

            let value = entry.value.clone();

            poi_tags
                .entry(value)
                .or_default()
                .push((entry.key, entry.value));
        } else if entry.table == "landcovers"
            && matches!(
                entry.kind,
                MappingKind::TableMapping | MappingKind::TableMappingNested
            )
        {
            landcover_tags.insert(entry.value, entry.key);
        }
    }

    let mut poi_groups: IndexMap<String, (Category, Vec<IndexMap<String, String>>, String)> =
        IndexMap::new();

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
            .entry(visual_key.to_string())
            .or_insert_with(|| (def.category, Vec::new(), typ.to_string()));

        entry.1.push(build_poi_tags(typ, &poi_tags));
    }

    let poi_items = poi_groups
        .into_iter()
        .map(|(visual_key, (category, tags, repr_typ))| {
            LegendItem::new(
                format!("poi_{}", visual_key),
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
            format!("landcover_{}", id_typ),
            Category::Landcover,
            tags,
            build_landcover_data(id_typ, 19),
            19,
        )
    });

    let mut legend_feature = HashMap::new();

    legend_feature.insert(
        "type".to_string(),
        LegendValue::String("tree_row".to_string()),
    );
    legend_feature.insert(
        "geometry".to_string(),
        LegendValue::LineString(LineString::new(vec![
            Coord { x: -60.0, y: -30.0 },
            Coord { x: 60.0, y: 30.0 },
        ])),
    );

    let mut legend_map: LegendItemData = HashMap::new();
    legend_map.insert("feature_lines".to_string(), vec![legend_feature]);

    let lines = vec![LegendItem::new(
        "tree_row".to_string(),
        Category::Water,
        vec![IndexMap::from([(
            "natural".to_string(),
            "tree_row".to_string(),
        )])],
        legend_map,
        17,
    )];

    poi_items.chain(landcover_items).chain(lines).collect()
});

pub fn legend_metadata() -> Vec<LegendMeta> {
    LEGEND_ITEMS.iter().map(|item| item.meta.clone()).collect()
}

// layer -> "tags"
pub type LegendItemData = HashMap<String, Vec<HashMap<String, LegendValue>>>;

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
    typ: &str,
    poi_tags: &HashMap<String, Vec<(String, String)>>,
) -> IndexMap<String, String> {
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
                let key = key.as_str();
                let value = value.as_str();

                if key == "information" {
                    tags.push(("tourism", key));
                }

                tags.push((key, value));
            }
        }
    }

    build_tags_map(tags)
}

fn build_landcover_tags(
    typ: &str,
    landcover_tags: &HashMap<String, String>,
) -> IndexMap<String, String> {
    let mut tags = vec![];

    if let Some(value) = landcover_tags.get(typ) {
        tags.push((value.as_str(), typ));
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

fn build_tags_map(tags: Vec<(&str, &str)>) -> IndexMap<String, String> {
    let mut t = IndexMap::with_capacity(tags.len());

    for (k, v) in tags {
        t.insert(k.to_string(), v.to_string());
    }

    t
}

fn build_poi_data(typ: &str, for_zoom: u8) -> LegendItemData {
    HashMap::from([
        (
            "landcovers".to_string(),
            vec![HashMap::from([
                ("type".to_string(), LegendValue::String("wood".to_string())),
                ("name".to_string(), LegendValue::String("".to_string())),
                (
                    "geometry".to_string(),
                    LegendValue::Geometry(geo::Geometry::Polygon(polygon(true, for_zoom))),
                ),
            ])],
        ),
        (
            "pois".to_string(),
            vec![HashMap::from([
                ("type".to_string(), LegendValue::String(typ.to_string())),
                ("name".to_string(), LegendValue::String("Test".to_string())),
                ("extra".to_string(), LegendValue::Hstore(HashMap::new())),
                (
                    "geometry".to_string(),
                    LegendValue::Point(Point::new(0.0, 0.0)),
                ),
            ])],
        ),
    ])
}

fn build_landcover_data(typ: &str, for_zoom: u8) -> LegendItemData {
    HashMap::from([(
        "landcovers".to_string(),
        vec![HashMap::from([
            ("type".to_string(), LegendValue::String(typ.to_string())),
            ("name".to_string(), LegendValue::String("Test".to_string())),
            (
                "geometry".to_string(),
                LegendValue::Geometry(geo::Geometry::Polygon(polygon(true, for_zoom))),
            ),
        ])],
    )])
}

fn polygon(skew: bool, for_zoom: u8) -> Polygon {
    let factor = (19.0 - for_zoom as f64).exp2();

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
