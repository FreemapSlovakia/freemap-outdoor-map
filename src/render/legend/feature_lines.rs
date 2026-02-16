use crate::render::{
    layers::Category,
    legend::{LegendItem, leak_str, mapping::MappingEntry},
};
use geo::Point;
use indexmap::IndexMap;
use std::collections::HashMap;

pub fn feature_lines(mapping_entries: &[MappingEntry]) -> Vec<LegendItem<'static>> {
    let groups: &[(&[&str], Category)] = &[
        (&["line"], Category::Other),
        (&["minor_line"], Category::Other),
        (&["cutline"], Category::Other),
        (&["pipeline"], Category::Other),
        (&["pipeline_under"], Category::Other),
        (&["tree_row"], Category::Other),
        (&["weir"], Category::Water),
        (&["dam"], Category::Water),
        (&["earth_bank"], Category::Terrain),
        (&["dyke"], Category::Terrain),
        (&["embankment"], Category::Terrain),
        (&["gully"], Category::Terrain),
        (&["cliff"], Category::Terrain),
        (
            &["runway", "taxiway", "parking_position", "taxilane"],
            Category::Other,
        ),
        (&["city_wall"], Category::Other),
        (&["hedge"], Category::Other),
        (
            &["ditch", "fence", "retaining_wall", "wall"],
            Category::Other,
        ),
        (
            &[
                "cable_car",
                "chair_lift",
                "drag_lift",
                "gondola",
                "goods",
                "j-bar",
                "magic_carpet",
                "mixed_lift",
                "platter",
                "rope_tow",
                "t-bar",
                "zip_line",
            ],
            Category::Other,
        ),
    ];

    groups
        .iter()
        .map(|(types, category)| {
            let zoom = match types[0] {
                "cutline" => 15,
                "hedge" => 18,
                _ => 17,
            };

            let mut item =
                LegendItem::builder(format!("line_{}", types[0]).leak(), *category, zoom)
                    .add_tag_set(|mut ts| {
                        for tag_set in types.iter().flat_map(|typ_| {
                            let typ = if *typ_ == "pipeline_under" {
                                "pipeline"
                            } else {
                                *typ_
                            };

                            let mut tags = IndexMap::new();

                            for entry in mapping_entries {
                                if entry.table == "feature_lines" && entry.value == typ {
                                    let value = leak_str(&entry.value);
                                    let key = leak_str(&entry.key);

                                    tags.insert(key, value);
                                }
                            }

                            let mut sets = vec![];

                            if *typ_ == "pipeline_under" {
                                tags.insert("location", "underwater");

                                let mut tags = tags.clone();
                                tags.insert("location", "underground");
                                sets.push(tags);
                            }

                            sets.push(tags);

                            if typ == "line" {
                                sets.push([("power", "tower")].into());
                            } else if typ == "minor_line" {
                                sets.push([("power", "pole")].into());
                            }

                            sets
                        }) {
                            ts = ts.add_tags(|mut tb| {
                                for (k, v) in &tag_set {
                                    tb = tb.add(k, v);
                                }
                                tb
                            });
                        }

                        ts
                    })
                    .add_feature("landcovers", |b| {
                        b.with("type", "meadow").with("name", "").with_polygon(true)
                    })
                    .add_feature("feature_lines", |b| {
                        b.with("name", if types[0] == "cable_car" { "Abc" } else { "" }) // NOTE only aerialways have name
                            .with(
                                "type",
                                if types[0] == "pipeline_under" {
                                    "pipeline"
                                } else {
                                    types[0]
                                },
                            )
                            .with("class", "highway")
                            .with(
                                "tags",
                                if types[0] == "pipeline_under" {
                                    HashMap::from([("location".into(), Some("underground".into()))])
                                } else {
                                    HashMap::new()
                                },
                            )
                            .with_line_string(false)
                    });

            if types[0] == "line" {
                item = item.add_feature("power_towers_poles", |b| {
                    b.with("type", "power_tower")
                        .with("geometry", Point::new(0.0, 0.0))
                });
            } else if types[0] == "minor_line" {
                item = item.add_feature("power_towers_poles", |b| {
                    b.with("type", "pole")
                        .with("geometry", Point::new(0.0, 0.0))
                });
            }

            item.build()
        })
        .collect()
}
