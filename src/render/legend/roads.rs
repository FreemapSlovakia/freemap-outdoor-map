use crate::render::{
    LegendValue,
    layers::Category,
    legend::{LegendItem, PropsBuilder},
};
use indexmap::IndexMap;

pub fn roads() -> Vec<LegendItem<'static>> {
    [
        &["motorway", "trunk"] as &[&str],
        &["primary", "motorway_link", "trunk_link"],
        &["secondary", "primary_link", ""],
        &["tertiary", "tertiary_link", "secondary_link"],
        &["residential", "unclassified", "living_street", "road"],
        &["footway", "pedestrian"],
        &["platform"],
        &["steps"],
        &["cycleway"],
        &["path"],
        &["piste"],
        &["bridleway"],
        &["via_ferrata"],
        &["track"],
    ]
    .iter()
    .enumerate()
    .map(|(i, types)| {
        LegendItem::builder(
            format!("road_{}", types[0]).leak(),
            Category::RoadsAndPaths,
            17,
        )
        .add_tag_set(|mut ts| {
            for typ in *types {
                if *typ == "platform" {
                    ts = ts.add_tags(|tags| {
                        tags.add("highway", "platform")
                            .add("railway", "platform")
                            .add("public_transport", "platform")
                    });
                } else {
                    ts = ts.add_tags(|tags| tags.add("highway", typ));
                }
            }

            ts
        })
        .add_feature("landcovers", |b| {
            b.with("type", if i < 10 { "residential" } else { "wood" })
                .with("name", "")
                .with_polygon(true)
        })
        .add_feature("roads", |b| b.with_road(types[0]).with("class", "highway"))
        .build()
    })
    .chain(
        ([
            &[("oneway", "yes")] as &[(&str, &str)],
            &[("foot", "no")],
            &[("bicycle", "no")],
            &[("foot", "no"), ("bicycle", "no")],
            &[("bridge", "yes")],
            &[("tunnel", "yes")],
        ])
        .into_iter()
        .map(|tags| {
            let road_type = match tags[0].0 {
                "highway" => "path",
                "tunnel" | "bridge" => "secondary",
                _ => "service",
            };

            LegendItem::builder(
                format!(
                    "road_{}",
                    tags.iter()
                        .map(|t| format!("{}_{}", t.0, t.1))
                        .collect::<String>()
                )
                .leak(),
                Category::RoadsAndPaths,
                17,
            )
            .add_tag_set(|ts| {
                ts.add_tags(|tags_builder| {
                    let mut tags_builder = tags_builder.add("highway", "*");
                    for (k, v) in tags {
                        tags_builder = tags_builder.add(k, v);
                    }
                    tags_builder
                })
            })
            .add_feature("landcovers", |b| {
                b.with("type", "wood").with("name", "").with_polygon(true)
            })
            .add_feature("roads", |b| {
                let mut b = b.with_road(road_type).with("class", "highway");

                for tag in tags {
                    if matches!(tag.0, "foot" | "bicycle") {
                        continue;
                    }

                    b = b.with(
                        tag.0,
                        if matches!(tag.0, "bridge" | "tunnel" | "oneway") {
                            LegendValue::I16(1)
                        } else {
                            LegendValue::String(tag.1)
                        },
                    );
                }

                b
            })
            .add_feature("road_access_restrictions", |b| {
                let mut no_foot = 0i32;
                let mut no_bicycle = 0i32;

                for tag in tags {
                    if tag.0 == "foot" {
                        no_foot = 1;
                    }

                    if tag.0 == "bicycle" {
                        no_bicycle = 1;
                    }
                }

                b.with_road(road_type)
                    .with("no_foot", no_foot)
                    .with("no_bicycle", no_bicycle)
            })
            .build()
        }),
    )
    .chain([
        LegendItem::builder("path_bike_foot", Category::RoadsAndPaths, 17)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| {
                    tags.add("highway", "path")
                        .add("foot", "designated")
                        .add("bicycle", "designated")
                })
            })
            .add_feature("landcovers", |b| {
                b.with("type", "residential")
                    .with("name", "")
                    .with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("path")
                    .with("class", "highway")
                    .with("foot", "designated")
                    .with("bicycle", "designated")
            })
            .build(),
        LegendItem::builder("road_construction", Category::RoadsAndPaths, 17)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("highway", "construction")))
            .add_feature("landcovers", |b| {
                b.with("type", "residential")
                    .with("name", "")
                    .with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("construction").with("class", "highway")
            })
            .build(),
        LegendItem::builder("route_hiking", Category::RoadsAndPaths, 17)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| {
                    tags.add("type", "route")
                        .add("route", "hiking")
                        .add("network", "rwn")
                })
                .add_tags(|tags| {
                    tags.add("type", "route")
                        .add("route", "hiking")
                        .add("network", "nwn")
                })
                .add_tags(|tags| {
                    tags.add("type", "route")
                        .add("route", "hiking")
                        .add("network", "iwn")
                })
            })
            .add_feature("landcovers", |b| {
                b.with("type", "wood").with("name", "").with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("track")
                    .with("name", "")
                    .with("class", "highway")
                    .with("tracktype", "grade3")
            })
            .add_feature("routes", |b| {
                b.with_route(false)
                    .with("refs1", "0901")
                    .with("off1", 1i32)
                    .with("h_red", 1i32)
            })
            .build(),
        LegendItem::builder("route_hiking_local", Category::RoadsAndPaths, 17)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| {
                    tags.add("type", "route")
                        .add("route", "hiking")
                        .add("network", "lwn")
                })
            })
            .add_feature("landcovers", |b| {
                b.with("type", "wood").with("name", "").with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("track")
                    .with("name", "")
                    .with("class", "highway")
                    .with("tracktype", "grade3")
            })
            .add_feature("routes", |b| {
                b.with_route(false)
                    .with("refs1", "M0123")
                    .with("off1", 1i32)
                    .with("h_red_loc", 1i32)
            })
            .build(),
        LegendItem::builder("route_bicycle", Category::RoadsAndPaths, 17)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| {
                    tags.add("type", "route")
                        .add("route", "bicycle")
                        .add("network", "lwn")
                })
            })
            .add_feature("landcovers", |b| {
                b.with("type", "wood").with("name", "").with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("track")
                    .with("name", "")
                    .with("class", "highway")
                    .with("tracktype", "grade3")
            })
            .add_feature("routes", |b| {
                b.with_route(true)
                    .with("refs2", "C12")
                    .with("off2", 1i32)
                    .with("b_red", 1i32)
            })
            .build(),
        LegendItem::builder("route_ski", Category::RoadsAndPaths, 17)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("type", "route").add("route", "ski")))
            .add_feature("landcovers", |b| {
                b.with("type", "wood").with("name", "").with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("track")
                    .with("name", "")
                    .with("class", "highway")
                    .with("tracktype", "grade3")
            })
            .add_feature("routes", |b| {
                b.with_route(true)
                    .with("refs2", "S12")
                    .with("off2", 1i32)
                    .with("s_red", 1i32)
            })
            .build(),
        LegendItem::builder("route_horse", Category::RoadsAndPaths, 17)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("type", "route").add("route", "horse")))
            .add_feature("landcovers", |b| {
                b.with("type", "wood").with("name", "").with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("track")
                    .with("name", "")
                    .with("class", "highway")
                    .with("tracktype", "grade3")
            })
            .add_feature("routes", |b| {
                b.with_route(false)
                    .with("refs1", "H12")
                    .with("off1", 1i32)
                    .with("r_red", 1i32)
            })
            .build(),
    ])
    .chain((1..=5).map(|grade| {
        let grade: &str = format!("grade{grade}").leak();

        LegendItem::builder(
            format!("road_track_{grade}").leak(),
            Category::RoadsAndPaths,
            17,
        )
        .add_tag_set(|mut ts| {
            ts = ts.add_tags(|tags| tags.add("highway", "track").add("tracktype", grade));

            if grade == "grade1" {
                ts = ts.add_tags(|tags| tags.add("highway", "service"));
            } else if grade == "grade2" {
                ts = ts.add_tags(|tags| tags.add("highway", "raceway"));
                ts = ts.add_tags(|tags| tags.add("leisure", "track"));
            }

            ts
        })
        .add_feature("landcovers", |b| {
            b.with("type", "wood").with("name", "").with_polygon(true)
        })
        .add_feature("roads", |b| {
            b.with_road("track")
                .with("class", "highway")
                .with("tracktype", grade)
        })
        .build()
    }))
    .chain(
        ["excellent", "good", "intermediate", "bad", "horrible", "no"]
            .into_iter()
            .enumerate()
            .map(|(i, visibility)| {
                LegendItem::builder(
                    format!("trail_visibility_{visibility}").leak(),
                    Category::RoadsAndPaths,
                    17,
                )
                .add_tag_set(|ts| ts.add_tags(|tags| tags.add("trail_visibility", visibility)))
                .add_feature("landcovers", |b| {
                    b.with("type", "wood").with("name", "").with_polygon(true)
                })
                .add_feature("roads", |b| {
                    b.with_road("path")
                        .with("class", "highway")
                        .with("trail_visibility", i as i32)
                })
                .build()
            }),
    )
    .chain(
        [
            &["rail"] as &[&str],
            &["light_rail", "tram"], // || typ == "rail" && service != "main" && !service.is_empty()
            &[
                "miniature",
                "monorail",
                "funicular",
                "narrow_gauge",
                "subway",
            ],
            &["disused", "preserved"],
            &["construction"],
        ]
        .iter()
        .map(|types| {
            LegendItem::builder(
                format!("railway_{}", types[0]).leak(),
                Category::Railway,
                17,
            )
            .add_tag_set(|mut ts| {
                for tag_set in types.iter().flat_map(|typ| match *typ {
                    "rail" => vec![
                        IndexMap::from([("railway", "rail")]),
                        IndexMap::from([("railway", "rail"), ("service", "main")]),
                    ],
                    "light_rail" => vec![
                        IndexMap::from([("railway", "light_rail")]),
                        IndexMap::from([("railway", "rail"), ("service", "≠main")]),
                    ],
                    _ => vec![IndexMap::from([("railway", *typ)])],
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
                b.with("type", "residential")
                    .with("name", "")
                    .with_polygon(true)
            })
            .add_feature("roads", |b| b.with_road(types[0]).with("class", "railway"))
            .build()
        }),
    )
    .chain([
        LegendItem::builder("railway_bridge", Category::Railway, 17)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("railway", "rail").add("bridge", "yes")))
            .add_feature("landcovers", |b| {
                b.with("type", "residential")
                    .with("name", "")
                    .with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("rail")
                    .with("class", "railway")
                    .with("bridge", 1i16)
            })
            .build(),
        LegendItem::builder("railway_tunnel", Category::Railway, 17)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("railway", "rail").add("tunnel", "yes")))
            .add_feature("landcovers", |b| {
                b.with("type", "residential")
                    .with("name", "")
                    .with_polygon(true)
            })
            .add_feature("roads", |b| {
                b.with_road("rail")
                    .with("class", "railway")
                    .with("tunnel", 1i16)
            })
            .build(),
        LegendItem::builder("water_slide", Category::Other, 17)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("attraction", "water_slide")))
            .add_feature("roads", |b| {
                b.with_road("water_slide").with("class", "attraction")
            })
            .build(),
    ])
    .collect()
}

impl PropsBuilder {
    fn with_road(self, typ: &'static str) -> Self {
        self.with("type", typ)
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
            .with_line_string(false)
    }

    fn with_route(self, reverse: bool) -> Self {
        self.with_line_string(reverse)
            .with("refs1", "")
            .with("off1", 0i32)
            .with("refs2", "")
            .with("off2", 0i32)
            .with("h_red", 0i32)
            .with("h_blue", 0i32)
            .with("h_green", 0i32)
            .with("h_yellow", 0i32)
            .with("h_black", 0i32)
            .with("h_white", 0i32)
            .with("h_orange", 0i32)
            .with("h_purple", 0i32)
            .with("h_none", 0i32)
            .with("h_red_loc", 0i32)
            .with("h_blue_loc", 0i32)
            .with("h_green_loc", 0i32)
            .with("h_yellow_loc", 0i32)
            .with("h_black_loc", 0i32)
            .with("h_white_loc", 0i32)
            .with("h_orange_loc", 0i32)
            .with("h_purple_loc", 0i32)
            .with("h_none_loc", 0i32)
            .with("b_red", 0i32)
            .with("b_blue", 0i32)
            .with("b_green", 0i32)
            .with("b_yellow", 0i32)
            .with("b_black", 0i32)
            .with("b_white", 0i32)
            .with("b_orange", 0i32)
            .with("b_purple", 0i32)
            .with("b_none", 0i32)
            .with("s_red", 0i32)
            .with("s_blue", 0i32)
            .with("s_green", 0i32)
            .with("s_yellow", 0i32)
            .with("s_black", 0i32)
            .with("s_white", 0i32)
            .with("s_orange", 0i32)
            .with("s_purple", 0i32)
            .with("s_none", 0i32)
            .with("r_red", 0i32)
            .with("r_blue", 0i32)
            .with("r_green", 0i32)
            .with("r_yellow", 0i32)
            .with("r_black", 0i32)
            .with("r_white", 0i32)
            .with("r_orange", 0i32)
            .with("r_purple", 0i32)
            .with("r_none", 0i32)
            .with("class", "highway")
            .with("tracktype", "grade3")
    }
}
