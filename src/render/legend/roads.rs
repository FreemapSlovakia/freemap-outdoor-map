use crate::render::{
    LegendValue,
    layers::Category,
    legend::{
        LegendItem,
        shared::{
            LegendFeatureDataBuilder, legend_feature_data_builder, legend_item_data_builder,
            with_landcover,
        },
    },
};
use indexmap::IndexMap;
use std::collections::HashMap;

pub fn roads() -> Vec<LegendItem<'static>> {
    [
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
        // &["raceway"],
    ]
    .iter()
    .enumerate()
    .map(|(i, types)| {
        LegendItem::new(
            format!("road_{}", types[0]).leak(),
            Category::RoadsAndPaths,
            types
                .iter()
                .flat_map(|typ| {
                    if *typ == "raceway" {
                        vec![
                            IndexMap::from([("highway", "raceway")]),
                            IndexMap::from([("leisure", "track")]),
                        ]
                    } else {
                        vec![IndexMap::from([("highway", *typ)])]
                    }
                })
                .collect::<Vec<_>>(),
            with_landcover(if i < 10 { "residential" } else { "wood" }, 17)
                .with_feature(
                    "roads",
                    road_builder(types[0], 17).with("class", "highway").build(),
                )
                .build(),
            17,
        )
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

            LegendItem::new(
                format!(
                    "road_{}",
                    tags.iter()
                        .map(|t| format!("{}_{}", t.0, t.1))
                        .collect::<String>()
                )
                .leak(),
                Category::RoadsAndPaths,
                vec![{
                    let mut map = IndexMap::from([("highway", "*")]);
                    map.extend(tags.iter().copied());
                    map
                }],
                {
                    let mut b = road_builder(road_type, 17).with("class", "highway");

                    let mut no_foot = 0i32;
                    let mut no_bicycle = 0i32;

                    for tag in tags {
                        if tag.0 == "foot" {
                            no_foot = 1;
                            continue;
                        }

                        if tag.0 == "bicycle" {
                            no_bicycle = 1;
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

                    with_landcover("wood", 17)
                        .with_feature("roads", b.build())
                        .with_feature(
                            "road_access_restrictions",
                            road_builder(road_type, 17)
                                .with("no_foot", no_foot)
                                .with("no_bicycle", no_bicycle)
                                .build(),
                        )
                        .build()
                },
                17,
            )
        }),
    )
    .chain([
        LegendItem::new(
            "path_bike_foot",
            Category::RoadsAndPaths,
            vec![
                [
                    ("highway", "path"),
                    ("foot", "designated"),
                    ("bicycle", "designated"),
                ]
                .into(),
            ],
            with_landcover("residential", 17)
                .with_feature(
                    "roads",
                    road_builder("path", 17)
                        .with("class", "highway")
                        .with("foot", "designated")
                        .with("bicycle", "designated")
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "road_construction",
            Category::RoadsAndPaths,
            vec![[("highway", "construction")].into()],
            with_landcover("residential", 17)
                .with_feature(
                    "roads",
                    road_builder("construction", 17)
                        .with("class", "highway")
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "route_hiking",
            Category::RoadsAndPaths,
            vec![
                [("type", "route"), ("route", "hiking"), ("network", "rwn")].into(),
                [("type", "route"), ("route", "hiking"), ("network", "nwn")].into(),
                [("type", "route"), ("route", "hiking"), ("network", "iwn")].into(),
            ],
            with_route(
                route_builder(17, false)
                    .with("refs1", "0901")
                    .with("off1", 1i32)
                    .with("h_red", 1i32)
                    .build(),
            ),
            17,
        ),
        LegendItem::new(
            "route_hiking_local",
            Category::RoadsAndPaths,
            vec![[("type", "route"), ("route", "hiking"), ("network", "lwn")].into()],
            with_route(
                route_builder(17, false)
                    .with("refs1", "M0123")
                    .with("off1", 1i32)
                    .with("h_red_loc", 1i32)
                    .build(),
            ),
            17,
        ),
        LegendItem::new(
            "route_bicycle",
            Category::RoadsAndPaths,
            vec![[("type", "route"), ("route", "bicycle"), ("network", "lwn")].into()],
            with_route(
                route_builder(17, true)
                    .with("refs2", "C12")
                    .with("off2", 1i32)
                    .with("b_red", 1i32)
                    .build(),
            ),
            17,
        ),
        LegendItem::new(
            "route_ski",
            Category::RoadsAndPaths,
            vec![[("type", "route"), ("route", "ski")].into()],
            with_route(
                route_builder(17, true)
                    .with("refs2", "S12")
                    .with("off2", 1i32)
                    .with("s_red", 1i32)
                    .build(),
            ),
            17,
        ),
        LegendItem::new(
            "route_horse",
            Category::RoadsAndPaths,
            vec![[("type", "route"), ("route", "horse")].into()],
            with_route(
                route_builder(17, false)
                    .with("refs1", "H12")
                    .with("off1", 1i32)
                    .with("r_red", 1i32)
                    .build(),
            ),
            17,
        ),
    ])
    .chain((1..=5).map(|grade| {
        let grade: &str = format!("grade{grade}").leak();

        LegendItem::new(
            format!("road_track_{grade}").leak(),
            Category::RoadsAndPaths,
            {
                let mut tags = vec![[("highway", "track"), ("tracktype", grade)].into()];

                if grade == "grade2" {
                    tags.push([("highway", "raceway")].into());
                }

                tags
            },
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
    }))
    .chain(
        ["excellent", "good", "intermediate", "bad", "horrible", "no"]
            .into_iter()
            .enumerate()
            .map(|(i, visibility)| {
                LegendItem::new(
                    format!("trail_visibility_{visibility}").leak(),
                    Category::RoadsAndPaths,
                    vec![[("trail_visibility", visibility)].into()],
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
            LegendItem::new(
                format!("railway_{}", types[0]).leak(),
                Category::Railway,
                types
                    .iter()
                    .flat_map(|typ| match *typ {
                        "rail" => vec![
                            IndexMap::from([("railway", "rail")]),
                            IndexMap::from([("railway", "rail"), ("service", "main")]),
                        ],
                        "light_rail" => vec![
                            IndexMap::from([("railway", "light_rail")]),
                            IndexMap::from([("railway", "rail"), ("service", "≠main")]),
                        ],
                        _ => vec![IndexMap::from([("railway", *typ)])],
                    })
                    .collect::<Vec<_>>(),
                with_landcover("residential", 17)
                    .with_feature(
                        "roads",
                        road_builder(types[0], 17).with("class", "railway").build(),
                    )
                    .build(),
                17,
            )
        }),
    )
    .chain([
        LegendItem::new(
            "railway_bridge",
            Category::Railway,
            vec![[("railway", "rail"), ("bridge", "yes")].into()],
            with_landcover("residential", 17)
                .with_feature(
                    "roads",
                    road_builder("rail", 17)
                        .with("class", "railway")
                        .with("bridge", 1i16)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "railway_tunnel",
            Category::Railway,
            vec![[("railway", "rail"), ("tunnel", "yes")].into()],
            with_landcover("residential", 17)
                .with_feature(
                    "roads",
                    road_builder("rail", 17)
                        .with("class", "railway")
                        .with("tunnel", 1i16)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "water_slide",
            Category::Other,
            vec![[("attraction", "water_slide")].into()],
            legend_item_data_builder()
                .with_feature(
                    "roads",
                    road_builder("water_slide", 17)
                        .with("class", "attraction")
                        .build(),
                )
                .build(),
            17,
        ),
    ])
    .collect()
}

fn with_route(
    rt: HashMap<String, LegendValue>,
) -> HashMap<String, Vec<HashMap<String, LegendValue>>> {
    with_landcover("wood", 17)
        .with_feature(
            "roads",
            road_builder("track", 17)
                .with("name", "")
                .with("class", "highway")
                .with("tracktype", "grade3")
                .build(),
        )
        .with_feature("routes", rt)
        .build()
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
        .with_line_string(zoom, false)
}

fn route_builder(zoom: u8, reverse: bool) -> LegendFeatureDataBuilder {
    legend_feature_data_builder()
        .with_line_string(zoom, reverse)
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
