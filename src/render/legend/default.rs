use super::mapping;
use super::shared::{legend_feature_data_builder, road_builder, with_landcover};
use super::{LegendItem, mapping_path};
use crate::render::layers::Category;
use crate::render::legend::landcovers::landcovers;
use crate::render::legend::pois::pois;
use indexmap::IndexMap;
use mapping::collect_mapping_entries;
use std::io::BufReader;

pub(super) fn build_default_legend_items() -> Vec<LegendItem<'static>> {
    let mapping_root: mapping::MappingRoot = {
        let mapping_file = std::fs::File::open(mapping_path()).expect("read mapping.yaml");

        serde_saphyr::from_reader(BufReader::new(mapping_file)).expect("parse mapping.yaml")
    };

    let mapping_entries = collect_mapping_entries(&mapping_root);

    let other = vec![
        LegendItem::new(
            "line_tree_row",
            Category::NaturalPoi,
            [[("natural", "tree_row")].into()],
            with_landcover("farmland", 17)
                .with_feature(
                    "feature_lines",
                    legend_feature_data_builder()
                        .with("type", "tree_row")
                        .with_line_string(17)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "barrier_other",
            Category::Other,
            [[("barrier", "*")].into()],
            with_landcover("meadow", 17)
                .with_feature(
                    "barrierways",
                    legend_feature_data_builder()
                        .with("type", "")
                        .with_line_string(17)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "barrier_city_wall",
            Category::Other,
            [[("barrier", "city_wall")].into()],
            with_landcover("residential", 17)
                .with_feature(
                    "barrierways",
                    legend_feature_data_builder()
                        .with("type", "city_wall")
                        .with_line_string(17)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "barrier_hedge",
            Category::Other,
            [[("barrier", "hedge")].into()],
            with_landcover("residential", 19)
                .with_feature(
                    "barrierways",
                    legend_feature_data_builder()
                        .with("type", "hedge")
                        .with_line_string(19)
                        .build(),
                )
                .build(),
            19,
        ),
        LegendItem::new(
            "line_weir",
            Category::Water,
            [[("waterway", "weir")].into()],
            with_landcover("water", 17) // TODO there is no water actually
                .with_feature(
                    "feature_lines",
                    legend_feature_data_builder()
                        .with("type", "weir")
                        .with_line_string(17)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "line_dam",
            Category::Water,
            [[("waterway", "dam")].into()],
            with_landcover("water", 17) // TODO there is no water actually
                .with_feature(
                    "feature_lines",
                    legend_feature_data_builder()
                        .with("type", "dam")
                        .with_line_string(17)
                        .build(),
                )
                .build(),
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

    let poi_items = pois(&mapping_root, &mapping_entries);

    let landcover_items = landcovers(&mapping_entries);

    poi_items
        .into_iter()
        .chain(landcover_items)
        .chain(roads)
        .chain(tracks)
        .chain(visibilities)
        .chain(other)
        .collect()
}
