use super::mapping;
use super::shared::{legend_feature_data_builder, with_landcover};
use super::{LegendItem, mapping_path};
use crate::render::{
    layers::Category,
    legend::{landcovers::landcovers, pois::pois, roads::roads, shared::leak_str},
};
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

    let poi_items = pois(&mapping_root, &mapping_entries);

    let landcover_items = landcovers(&mapping_entries);

    let roads = roads();

    poi_items
        .into_iter()
        .chain(landcover_items)
        .chain(roads)
        .chain(lines)
        .collect()
}
