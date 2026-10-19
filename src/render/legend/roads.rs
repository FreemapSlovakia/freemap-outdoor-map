use crate::render::{
    Feature, LegendValue, RenderLayer,
    layers::{Category, ROUTE_COLORS, road_access_restrictions::forbids, roads::RoadStyle},
    legend::{BuildOpts, LegendItem, Lend, PropsBuilder, Row, TagsBuilder},
};
use indexmap::IndexMap;
use std::fmt::Write as _;

/// Route markings are gated in `layers::pipeline` — zoom 8 with `RoutesHikingKst`, which a
/// legend render never asks for, otherwise zoom 9.
const ROUTE_MARKING_FROM_ZOOM: u8 = 9;

/// Lowest zoom at which `layers::roads::render` draws a way of this class and type — see the
/// `match (zoom, class, typ)` there. Paths, tracks and the like are also drawn at zoom 12
/// when the way is part of a route, which a legend sample never is.
fn road_from_zoom(class: &str, typ: &str) -> u8 {
    match (class, typ) {
        ("railway", "rail") => 8,
        (
            "railway",
            "light_rail" | "tram" | "miniature" | "monorail" | "funicular" | "narrow_gauge"
            | "subway" | "construction",
        ) => 13,
        ("railway", _) => 14, // disused, preserved
        (
            _,
            "motorway" | "trunk" | "motorway_link" | "trunk_link" | "primary" | "primary_link"
            | "secondary" | "secondary_link" | "tertiary" | "tertiary_link",
        ) => 8,
        (
            _,
            "residential" | "unclassified" | "living_street" | "road" | "construction" | "service"
            | "bridge" | "tunnel",
        ) => 12,
        (_, "cycleway" | "path" | "bridleway" | "via_ferrata" | "track") => 13,
        _ => 14, // footway, pedestrian, platform, steps, piste, water_slide
    }
}

pub fn roads(opts: BuildOpts) -> Vec<LegendItem<'static>> {
    [
        &["motorway", "trunk", "motorway_link", "trunk_link"] as &[&str],
        &["primary"],
        &["secondary", "primary_link"],
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
            opts,
        )
        .add_tag_set(|mut ts| {
            for typ in *types {
                if *typ == "platform" {
                    ts = ts
                        .add_tags(|tags| tags.add("highway", "platform"))
                        .add_tags(|tags| tags.add("railway", "platform"))
                        .add_tags(|tags| tags.add("public_transport", "platform"));
                } else {
                    ts = ts.add_tags(|tags| tags.add("highway", typ));
                }
            }

            ts
        })
        .min_zoom(road_from_zoom("highway", types[0]))
        .present_when("roads", &[], move |row| row.drawn_as(road_styles(types[0])))
        .named_by("highway_names", types, Lend::NAME)
        .add_landcover(if i < 10 { "residential" } else { "wood" })
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
            let access = access_presence(tags);

            let road_type = match tags[0].0 {
                "highway" => "path",
                "tunnel" | "bridge" => "secondary",
                _ => "service",
            };

            // The way underneath is drawn earlier than the detail this entry is about, so the
            // lower edge is not something a render can be probed for: bridge and tunnel casings
            // appear with the road class itself, oneway arrows and access restriction markers
            // only from zoom 14.
            let from_zoom = match tags[0].0 {
                // `draw_bridges_tunnels` is only reached from the zoom 12+ arms of the match.
                "bridge" | "tunnel" => 12,
                _ => 14,
            };

            LegendItem::builder(
                format!(
                    "road_{}",
                    tags.iter().fold(String::new(), |mut out, t| {
                        let _ = write!(out, "{}_{}", t.0, t.1);
                        out
                    })
                )
                .leak(),
                Category::RoadsAndPaths,
                17,
                opts,
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
            .min_zoom_unprobeable(from_zoom)
            .present_when(access.0, &[], access.1)
            .add_landcover("wood")
            .add_feature("roads", |b| {
                let mut b = b.with_road(road_type).with("class", "highway");

                for tag in tags {
                    if matches!(tag.0, "foot" | "bicycle" | "oneway") {
                        continue;
                    }

                    b = b.with(
                        tag.0,
                        if matches!(tag.0, "bridge" | "tunnel") {
                            LegendValue::I16(1)
                        } else {
                            LegendValue::String(tag.1)
                        },
                    );
                }

                b
            })
            // Open parts either side give the sample its bars; off-centre keeps its symbol clear
            // of the name label.
            .add_feature("road_access_restrictions", |b| {
                access_part(b, road_type, &[], 0.0, 0.5)
            })
            .add_feature("road_access_restrictions", |b| {
                access_part(b, road_type, tags, 0.5, 0.8)
            })
            .add_feature("road_access_restrictions", |b| {
                access_part(b, road_type, &[], 0.8, 1.0)
            })
            .build()
        }),
    )
    .chain([
        LegendItem::builder("path_bike_foot", Category::RoadsAndPaths, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| {
                    tags.add("highway", "path")
                        .add("foot", "designated")
                        .add("bicycle", "designated")
                })
            })
            .min_zoom(road_from_zoom("highway", "path"))
            .present_when("roads", &["path"], |row| {
                row.drawn_as(&[RoadStyle::PathBikeFoot])
            })
            .add_landcover("residential")
            .add_feature("roads", |b| {
                b.with_road("path")
                    .with("class", "highway")
                    .with("foot", "designated")
                    .with("bicycle", "designated")
            })
            .build(),
        LegendItem::builder("road_construction", Category::RoadsAndPaths, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("highway", "construction")))
            .min_zoom(road_from_zoom("highway", "construction"))
            .present_when("roads", &["construction"], |row| {
                row.drawn_as(&[RoadStyle::Construction])
            })
            .add_landcover("residential")
            .add_feature("roads", |b| {
                b.with_road("construction").with("class", "highway")
            })
            .build(),
    ])
    .chain(ROUTE_KINDS.iter().map(|kind| route_item(kind, opts)))
    .chain((1..=5).map(|grade| {
        let grade: &str = format!("grade{grade}").leak();

        LegendItem::builder(
            format!("road_track_{grade}").leak(),
            Category::RoadsAndPaths,
            17,
            opts,
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
        // Only grade1 tracks are firm enough to be drawn one zoom earlier than the rest.
        .min_zoom(if grade == "grade1" {
            12
        } else {
            road_from_zoom("highway", "track")
        })
        // A service road is drawn as a grade1 track, a raceway as a grade2 one.
        .present_when("roads", &[], move |row| {
            row.drawn_as(&[RoadStyle::Track])
                && row
                    .get_string("tracktype")
                    .is_ok_and(|tracktype| tracktype == grade)
                || grade == "grade1" && row.drawn_as(&[RoadStyle::Service])
                || grade == "grade2" && row.drawn_as(&[RoadStyle::Raceway])
        })
        .add_landcover("wood")
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
                    opts,
                )
                .add_tag_set(|ts| ts.add_tags(|tags| tags.add("trail_visibility", visibility)))
                .min_zoom(road_from_zoom("highway", "path"))
                // The column is imposm's 1-based enumeration, 0 where the tag is absent.
                .present_when("roads", &[], move |row| {
                    row.get_i32("trail_visibility")
                        .is_ok_and(|value| value == i as i32 + 1)
                })
                .add_landcover("wood")
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
                opts,
            )
            .min_zoom(road_from_zoom("railway", types[0]))
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
            .present_when("roads", &[], move |row| {
                row.drawn_as(railway_styles(types[0]))
            })
            .add_landcover("residential")
            .add_feature("roads", |b| b.with_road(types[0]).with("class", "railway"))
            .build()
        }),
    )
    .chain([
        LegendItem::builder("railway_bridge", Category::Railway, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("railway", "rail").add("bridge", "yes")))
            .min_zoom(road_from_zoom("railway", "rail"))
            .present_when("roads", &[], |row| {
                row.drawn_as(RAIL) && row.get_i16("bridge").is_ok_and(|bridge| bridge != 0)
            })
            .add_landcover("residential")
            .add_feature("roads", |b| {
                b.with_road("rail")
                    .with("class", "railway")
                    .with("bridge", 1i16)
            })
            .build(),
        LegendItem::builder("railway_tunnel", Category::Railway, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("railway", "rail").add("tunnel", "yes")))
            .min_zoom(road_from_zoom("railway", "rail"))
            .present_when("roads", &[], |row| {
                row.drawn_as(RAIL) && row.get_i16("tunnel").is_ok_and(|tunnel| tunnel != 0)
            })
            .add_landcover("residential")
            .add_feature("roads", |b| {
                b.with_road("rail")
                    .with("class", "railway")
                    .with("tunnel", 1i16)
            })
            .build(),
        LegendItem::builder("water_slide", Category::Sport, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("attraction", "water_slide")))
            .min_zoom(road_from_zoom("attraction", "water_slide"))
            .present_when("roads", &["water_slide"], |row| {
                row.drawn_as(&[RoadStyle::WaterSlide])
            })
            .add_feature("roads", |b| {
                b.with_road("water_slide").with("class", "attraction")
            })
            .build(),
    ])
    .collect()
}

/// The styles the road item named after `typ` stands for.
fn road_styles(typ: &str) -> &'static [RoadStyle] {
    match typ {
        // Links are drawn in the motorway colour.
        "motorway" => &[
            RoadStyle::Motorway,
            RoadStyle::MotorwayLink,
            RoadStyle::MotorwayLow,
        ],
        "primary" => &[RoadStyle::Primary, RoadStyle::PrimaryLow],
        "secondary" => &[RoadStyle::Secondary, RoadStyle::SecondaryLow],
        "tertiary" => &[RoadStyle::Tertiary, RoadStyle::TertiaryLow],
        "residential" => &[RoadStyle::Residential, RoadStyle::ResidentialLow],
        "footway" => &[RoadStyle::Footway],
        "platform" => &[RoadStyle::Platform],
        "steps" => &[RoadStyle::Steps],
        "cycleway" => &[RoadStyle::Cycleway],
        "path" => &[RoadStyle::Path],
        "piste" => &[RoadStyle::Piste],
        "bridleway" => &[RoadStyle::Bridleway],
        "via_ferrata" => &[RoadStyle::ViaFerrata],
        "track" => &[RoadStyle::Track],
        _ => &[],
    }
}

const RAIL: &[RoadStyle] = &[RoadStyle::Rail, RoadStyle::RailLow];

/// The styles the railway item named after `typ` stands for.
fn railway_styles(typ: &str) -> &'static [RoadStyle] {
    match typ {
        "rail" => RAIL,
        "light_rail" => &[RoadStyle::LightRail],
        "miniature" => &[RoadStyle::MinorRail],
        "disused" => &[RoadStyle::DisusedRail],
        "construction" => &[RoadStyle::RailConstruction],
        _ => &[],
    }
}

fn is_class(row: &Feature, class: &str) -> bool {
    row.get_string("class").is_ok_and(|value| value == class)
}

/// A route kind with an item of its own.
struct RouteKind {
    id: &'static str,
    requires: &'static [RenderLayer],
    route: &'static str,
    /// The `network` values its tag sets list, if they are split by network.
    networks: &'static [&'static str],
    /// Its marking columns are `{prefix}{colour}{suffix}`, one per route colour.
    prefix: &'static str,
    suffix: &'static str,
    /// The `routes_labels` column holding its refs alone.
    refs: &'static str,
    /// Drawn on the right of the way, with `refs2`/`off2`.
    right: bool,
    sample_ref: &'static str,
}

const ROUTE_KINDS: [RouteKind; 5] = [
    RouteKind {
        id: "route_hiking",
        requires: &[RenderLayer::RoutesHiking, RenderLayer::RoutesHikingKst],
        route: "hiking",
        networks: &["rwn", "nwn", "iwn"],
        prefix: "h_",
        suffix: "",
        refs: "refs_hiking",
        right: false,
        sample_ref: "0901",
    },
    RouteKind {
        id: "route_hiking_local",
        requires: &[RenderLayer::RoutesHiking, RenderLayer::RoutesHikingKst],
        route: "hiking",
        networks: &["lwn"],
        prefix: "h_",
        suffix: "_loc",
        refs: "refs_hiking_loc",
        right: false,
        sample_ref: "M0123",
    },
    RouteKind {
        id: "route_bicycle",
        requires: &[RenderLayer::RoutesBicycle],
        route: "bicycle",
        networks: &["lwn"],
        prefix: "b_",
        suffix: "",
        refs: "refs_bicycle",
        right: true,
        sample_ref: "C12",
    },
    RouteKind {
        id: "route_ski",
        requires: &[RenderLayer::RoutesSki],
        route: "ski",
        networks: &[],
        prefix: "s_",
        suffix: "",
        refs: "refs_ski",
        right: true,
        sample_ref: "S12",
    },
    RouteKind {
        id: "route_horse",
        requires: &[RenderLayer::RoutesHorse],
        route: "horse",
        networks: &[],
        prefix: "r_",
        suffix: "",
        refs: "refs_horse",
        right: false,
        sample_ref: "H12",
    },
];

/// The colour route samples are marked in.
const SAMPLE_COLOR: &str = "red";

/// The items of route `refs` whose `osmc:symbol` is of [`SAMPLE_COLOR`], so the
/// symbol drawn matches the sample's marking.
fn sample_colored(refs: &str) -> String {
    refs.split('\n')
        .filter(|item| {
            item.split('\t')
                .nth(2)
                .and_then(|symbol| symbol.split(':').next())
                == Some(SAMPLE_COLOR)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn route_tags<'a>(tags: TagsBuilder<'a>, route: &'static str) -> TagsBuilder<'a> {
    tags.add("type", "route").add("route", route)
}

fn route_item(kind: &RouteKind, opts: BuildOpts) -> LegendItem<'static> {
    let column = |color: &str| format!("{}{color}{}", kind.prefix, kind.suffix);

    let marking: Vec<String> = ROUTE_COLORS
        .iter()
        .map(|(color, _)| column(color))
        .collect();

    let (refs, off) = if kind.right {
        ("refs2", "off2")
    } else {
        ("refs1", "off1")
    };

    LegendItem::builder(kind.id, Category::RoadsAndPaths, 17, opts)
        .requires(kind.requires)
        .add_tag_set(|mut ts| {
            if kind.networks.is_empty() {
                ts = ts.add_tags(|tags| route_tags(tags, kind.route));
            }

            for network in kind.networks {
                ts = ts.add_tags(|tags| route_tags(tags, kind.route).add("network", network));
            }

            ts
        })
        .min_zoom(ROUTE_MARKING_FROM_ZOOM)
        .present_when("routes_marking", &[], move |row| {
            marking
                .iter()
                .any(|column| row.get_i32(column).is_ok_and(|off| off > 0))
        })
        .refs_from("routes_labels", kind.refs, refs, sample_colored)
        .add_landcover("wood")
        .add_feature("roads", |b| {
            b.with_road("track")
                .with("name", "")
                .with("class", "highway")
                .with("tracktype", "grade3")
        })
        .add_feature("routes", |b| {
            b.with_route(kind.right)
                .with(refs, kind.sample_ref)
                .with(off, 1i32)
                .with(column(SAMPLE_COLOR), 1i32)
        })
        .build()
}

/// The stage and rows showing the way detail `tags` are about: a bridge or tunnel
/// casing on any road, the rest marked by `road_access_restrictions`.
fn access_presence(
    tags: &'static [(&'static str, &'static str)],
) -> (&'static str, impl Fn(&Row) -> bool + Send + Sync + 'static) {
    let key = tags[0].0;

    // Each combination has a mark of its own.
    let forbidden = (
        tags.iter().any(|tag| tag.0 == "bicycle"),
        tags.iter().any(|tag| tag.0 == "foot"),
    );

    let stage = if matches!(key, "bridge" | "tunnel") {
        "roads"
    } else {
        "road_access_restrictions"
    };

    (stage, move |row: &Row| match key {
        "bridge" | "tunnel" => {
            is_class(row, "highway")
                && row.road_style().is_some_and(RoadStyle::has_casings)
                && row.get_i16(key).is_ok_and(|value| value != 0)
        }
        "oneway" => row.get_i16("oneway").is_ok_and(|oneway| oneway != 0),
        _ => forbids(row) == Some(forbidden),
    })
}

/// A part of the legend line for `road_access_restrictions`, restricted by `tags`.
fn access_part(
    b: PropsBuilder,
    road_type: &'static str,
    tags: &[(&'static str, &'static str)],
    from: f64,
    to: f64,
) -> PropsBuilder {
    let tag = |key: &str| tags.iter().find(|tag| tag.0 == key).map_or("", |tag| tag.1);

    b.with_road(road_type)
        .with_line_string_part(from, to)
        .with("access", "")
        .with("vehicle", "")
        .with("bicycle", tag("bicycle"))
        .with("foot", tag("foot"))
        .with("oneway", i16::from(tag("oneway") == "yes"))
}

impl PropsBuilder {
    fn with_road(self, typ: &'static str) -> Self {
        self.with("type", typ)
            .with_name()
            .with("tracktype", "")
            .with("class", "")
            .with("service", "")
            .with("bridge", 0i16)
            .with("tunnel", 0i16)
            .with("bicycle", "")
            .with("foot", "")
            .with("trail_visibility", 0)
            // Read by `layers::roads::render` at zoom <= 12; a legend sample is never part
            // of a route, so paths and tracks stay hidden there just like on the map.
            .with("is_in_route", false)
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
