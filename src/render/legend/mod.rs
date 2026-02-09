mod ctx_ext;

use crate::render::layers::{Category, Def, POIS};
use crate::render::{ImageFormat, LegendValue, RenderRequest};
use geo::{Coord, Point, Rect};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::LazyLock;

#[derive(Clone, Serialize)]
pub struct LegendMeta {
    pub id: &'static str,
    pub category: Category,
    pub tags: HashMap<String, String>,
}

struct LegendItem {
    meta: LegendMeta,
    data: LegendItemData,
}

fn li(
    id: &'static str,
    category: Category,
    tags: Vec<(&str, &str)>,
    data: LegendItemData,
) -> LegendItem {
    let tags = {
        let mut t = HashMap::with_capacity(tags.len());

        for (k, v) in tags {
            t.insert(k.to_string(), v.to_string());
        }

        t
    };

    LegendItem {
        meta: LegendMeta { id, category, tags },
        data,
    }
}

static LEGEND_ITEMS: LazyLock<Vec<LegendItem>> = LazyLock::new(|| {
    #[derive(Debug)]
    enum State {
        None,
        Tables,
        Features,
        TypeMappings,
        TmPoints,
        TmAny,
        TmAnyMappingsMappingKey(String),
    }

    let mut state: State = State::None;

    let mut poi_tags = HashMap::new();

    for line in BufReader::new(
        File::open("/home/martin/fm/freemap-outdoor-map/mapping.yaml")
            .expect("opened mapping.yaml"),
    )
    .lines()
    {
        let mut line = line.expect("line");

        if let Some(pos) = line.find('#') {
            line.truncate(pos);
        }

        state = match (&state, line.as_str()) {
            (State::None, "tables:") => State::Tables,
            (State::Tables, "  features:") => State::Features,
            (State::Features, "    type_mappings:") => State::TypeMappings,
            (State::TypeMappings | State::TmAny, "      points:") => State::TmPoints,
            (State::TypeMappings | State::TmPoints, "      any:") => State::TmAny,
            (State::TmAny | State::TmAnyMappingsMappingKey(_), _)
                if line.starts_with("              ") && &line[14..15] != " " =>
            {
                State::TmAnyMappingsMappingKey(line[14..line.len() - 1].to_string())
            }
            (State::TmAnyMappingsMappingKey(key), _) if line.starts_with("                - ") => {
                poi_tags.insert((&line[18..]).to_string(), key.to_string());
                state
            }
            (State::Features, _) if line.trim().len() > 2 && &line[2..3] != " " => State::None,
            _ => state,
        };
    }

    POIS.iter()
        .filter_map(|def| {
            let typ = *def.0;

            if typ == "guidepost_noname" || typ.starts_with("peak") && typ.len() == 5 {
                return None;
            }

            let mut tags = vec![];

            if typ.starts_with("tower_") {
                tags.push(("man_made", "tower"));
                tags.push(("tower:type", &typ[6..]));
            } else if typ.starts_with("mast_") {
                tags.push(("man_made", "mast"));
                tags.push(("tower:type", &typ[5..]));
            } else if matches!(
                typ,
                "convenience"
                    | "fuel"
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
                match typ {
                    "tree_protected" => {
                        tags.push(("natural", "tree"));
                        tags.push(("protected", "yes"));
                    }
                    "building_ruins" => {
                        tags.push(("building", "ruins"));
                    }
                    "building" | "ford" | "mountain_pass" => {
                        tags.push((typ, "yes"));
                    }
                    "generator_wind" => {
                        tags.push(("power", "generator"));
                        tags.push(("source", "wind")); // OR method = 'wind_turbine'
                    }
                    "church" | "chapel" | "synagogue" | "mosque" | "cathedral" => {
                        tags.push(("building", typ));
                    }

                    "disused_mine" => {
                        // TODO also for 'mine', 'mineshaft'
                        tags.push(("man_made", "adit"));
                        tags.push(("disused", "yes"));
                    }
                    _ => {
                        if let Some(value) = poi_tags.get(typ) {
                            tags.push(((*value).as_str(), typ));
                        }
                    }
                };
            }

            Some(li(
                def.0,
                def.1.get(0).unwrap().category,
                tags,
                build_poi_data(typ, def.1),
            ))
        })
        .collect()
});

pub fn legend_metadata() -> Vec<LegendMeta> {
    LEGEND_ITEMS.iter().map(|item| item.meta.clone()).collect()
}

pub type LegendItemData = HashMap<String, Vec<HashMap<String, LegendValue>>>;

pub fn legend_render_request(id: &str, scale: f64) -> Option<RenderRequest> {
    let zoom = 16;

    let bbox = Rect::new(
        Coord {
            x: -100.0,
            y: -100.0,
        },
        Coord { x: 100.0, y: 100.0 },
    );

    let legend_map = LEGEND_ITEMS
        .iter()
        .find(|item| item.meta.id == id)
        .map(|item| item.data.clone())?;

    let mut render_request = RenderRequest::new(bbox, zoom, scale, ImageFormat::Jpeg);
    render_request.legend = Some(legend_map);

    Some(render_request)
}

fn build_poi_data(typ: &str, def: &Vec<Def>) -> LegendItemData {
    let mut legend_map: LegendItemData = HashMap::new();
    let mut legend_feature = HashMap::new();

    legend_feature.insert("type".to_string(), LegendValue::String(typ.to_string()));
    legend_feature.insert("n".to_string(), LegendValue::String("Test".to_string()));
    legend_feature.insert("h".to_string(), LegendValue::Hstore(HashMap::new()));
    legend_feature.insert(
        "geometry".to_string(),
        LegendValue::Point(Point::new(1.0, 1.0)),
    );

    legend_map.insert("features".to_string(), vec![legend_feature]);

    legend_map
}
