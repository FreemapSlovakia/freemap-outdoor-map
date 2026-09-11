use crate::render::{
    layers::Category,
    legend::{BuildOpts, LegendItem, leak_str, mapping::MappingEntry},
};
use geo::Point;
use indexmap::IndexMap;

pub fn feature_lines(
    mapping_entries: &[MappingEntry],
    opts: BuildOpts,
) -> Vec<LegendItem<'static>> {
    let groups: &[(&[&str], Category)] = &[
        (&["line"], Category::ManMade),
        (&["minor_line"], Category::ManMade),
        (&["cutline"], Category::ManMade),
        (&["pipeline"], Category::ManMade),
        (&["pipeline_under"], Category::ManMade),
        (&["tree_row"], Category::NaturalPoi),
        (&["weir"], Category::Water),
        (&["dam"], Category::Water),
        (&["earth_bank"], Category::Terrain),
        (&["dyke"], Category::Terrain),
        (&["embankment"], Category::Terrain),
        (&["gully"], Category::Terrain),
        (&["cliff"], Category::Terrain),
        (
            &["runway", "taxiway", "parking_position", "taxilane"],
            Category::Transport,
        ),
        (&["city_wall"], Category::Historic),
        (&["hedge"], Category::Barrier),
        (
            &["ditch", "fence", "retaining_wall", "wall"],
            Category::Barrier,
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
            Category::Transport,
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

            // Lowest zoom at which `layers::feature_lines::render` has an arm for the type —
            // the stage gates in `layers::pipeline` are never stricter than these.
            let from_zoom = match types[0] {
                "runway" => 11,
                "pipeline" | "pipeline_under" | "cable_car" => 12,
                "cutline" | "tree_row" | "line" => 13,
                "minor_line" => 14,
                "earth_bank" | "dyke" | "embankment" | "gully" | "cliff" => 15,
                _ => 16, // weir, dam, city_wall, hedge, ditch …
            };

            let mut item =
                LegendItem::builder(format!("line_{}", types[0]).leak(), *category, zoom, opts)
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
                    .min_zoom(from_zoom)
                    .add_landcover("meadow")
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
                            .with("under", types[0] == "pipeline_under")
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
