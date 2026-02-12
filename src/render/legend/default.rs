use super::mapping;
use super::shared::{legend_feature_data_builder, road_builder, with_landcover};
use super::{LegendItem, mapping_path};
use crate::render::layers::Category;
use crate::render::legend::landcovers::landcovers;
use crate::render::legend::pois::pois;
use crate::render::legend::shared::leak_str;
use indexmap::IndexMap;
use mapping::collect_mapping_entries;
use std::collections::HashMap;
use std::io::BufReader;

pub(super) fn build_default_legend_items() -> Vec<LegendItem<'static>> {
    let mapping_root: mapping::MappingRoot = {
        let mapping_file = std::fs::File::open(mapping_path()).expect("read mapping.yaml");

        serde_saphyr::from_reader(BufReader::new(mapping_file)).expect("parse mapping.yaml")
    };

    let mapping_entries = collect_mapping_entries(&mapping_root);

    let lines = (&[
        &["cutline"] as &[&str],
        &["pipeline"],
        &["weir"],
        &["dam"],
        &["tree_row"],
        &["earth_bank"],
        &["dyke"],
        &["embankment"],
        &["gully"],
        &["cliff"],
        &["runway", "taxiway", "parking_position", "taxilane"],
        &["city_wall"],
        &["hedge"],
        &["ditch", "fence", "retaining_wall", "wall"],
        &["line"],
        &["minor_line"],
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
    ])
        .into_iter()
        .map(|types| {
            let zoom = match types[0] {
                "cutline" => 15,
                "hedge" => 18,
                _ => 17,
            };

            LegendItem::new(
                format!("line_{}", types[0]).leak(),
                Category::Communications,
                types
                    .iter()
                    .map(|typ| {
                        let mut tags = IndexMap::new();

                        for entry in &mapping_entries {
                            if entry.table == "feature_lines" && entry.value == *typ {
                                let value = leak_str(&entry.value);
                                let key = leak_str(&entry.key);

                                tags.insert(key, value);
                            }
                        }

                        tags
                    })
                    .collect::<Vec<_>>(),
                with_landcover("meadow", zoom)
                    .with_feature(
                        "feature_lines",
                        legend_feature_data_builder()
                            .with("name", if types[0] == "cable_car" { "Abc" } else { "" }) // NOTE only aerialways have name
                            .with("type", types[0])
                            .with("class", "highway")
                            .with("tags", HashMap::new())
                            .with_line_string(zoom)
                            .build(),
                    )
                    .build(),
                zoom,
            )
        });

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
        .chain(lines)
        .collect()
}
