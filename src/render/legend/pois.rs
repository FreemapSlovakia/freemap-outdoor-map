use crate::render::{
    layers::{Category, POI_ORDER, POIS},
    legend::{
        LegendItem, LegendItemData,
        mapping::{self, MappingEntry},
        shared::{build_tags_map, leak_str, legend_feature_data_builder, with_landcover},
    },
};
use geo::Point;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

pub fn pois(
    mapping_root: &mapping::MappingRoot,
    mapping_entries: &[MappingEntry],
) -> Vec<LegendItem<'static>> {
    let mut poi_tags: HashMap<&'static str, Vec<(&'static str, &'static str)>> = HashMap::new();
    let mut feature_alias_values: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    let mut feature_alias_catchall: HashSet<&'static str> = HashSet::new();

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

    for entry in mapping_entries {
        if entry.table != "pois" && entry.table != "sports"
            || feature_alias_catchall.contains(entry.key.as_str())
            || feature_alias_values
                .get(entry.key.as_str())
                .is_some_and(|values| values.contains(entry.value.as_str()))
        {
            continue;
        }

        let value = leak_str(&entry.value);
        let key = leak_str(&entry.key);

        poi_tags.entry(value).or_default().push((key, value));
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
        if *typ == "guidepost_noname" || typ.starts_with("peak") && typ.len() == 5 {
            continue;
        }

        let Some(defs) = POIS.get(*typ) else {
            continue;
        };

        let Some(def) = defs.iter().find(|def| def.is_active_at(19)) else {
            continue;
        };

        let visual_key = def.icon_key(typ);

        let entry = poi_groups
            .entry(visual_key)
            .or_insert_with(|| (def.category, Vec::new(), *typ));

        entry.1.push(build_poi_tags(typ, &poi_tags));
    }

    poi_groups
        .into_iter()
        .map(|(visual_key, (category, tags, repr_typ))| {
            LegendItem::new(
                format!("poi_{visual_key}").leak(),
                category,
                tags,
                build_poi_data(repr_typ, 19, HashMap::<String, Option<String>>::new()),
                19,
            )
        })
        .chain(
            [
                (("drinkable", "yes"), ("drinking_water", "yes")),
                (("drinkable", "no"), ("drinking_water", "no")),
                (("hot", "yes"), ("natural", "hot_spring")),
                (
                    ("water_characteristic", "mineral"),
                    ("water_characteristic", "mineral"),
                ),
                (("refitted", "yes"), ("refitted", "yes")),
                (("intermittent", "yes"), ("intermittent", "yes")),
            ]
            .map(|((prop_name, prop_value), (tag_key, tag_value))| {
                LegendItem::new(
                    format!("poi_spring_{tag_key}_{tag_value}").leak(),
                    Category::Water,
                    {
                        let mut tags = vec![
                            [
                                (
                                    "natural",
                                    if tag_value == "hot_spring" {
                                        "hot_spring"
                                    } else {
                                        "spring"
                                    },
                                ),
                                (tag_key, tag_value),
                            ]
                            .into(),
                        ];

                        if prop_name == "intermittent" {
                            tags.push([("natural", "spring"), ("seasonal", "yes")].into());
                        }

                        tags
                    },
                    build_poi_data(
                        "spring",
                        19,
                        HashMap::<String, Option<String>>::from([(
                            prop_name.to_string(),
                            Some(prop_value.to_string()),
                        )]),
                    ),
                    19,
                )
            }),
        )
        .collect()
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
                let (a, b) = s.split_once("_").unwrap();
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
        }

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

fn build_poi_data(
    typ: &'static str,
    zoom: u8,
    extra: HashMap<String, Option<String>>,
) -> LegendItemData {
    let factor = (19.0 - zoom as f64).exp2();

    with_landcover("wood", zoom)
        .with_feature(
            "pois",
            legend_feature_data_builder()
                .with("type", typ)
                .with("name", "Abc")
                .with("extra", extra)
                .with("geometry", Point::new(0.0, factor * -2.0))
                .build(),
        )
        .build()
}
