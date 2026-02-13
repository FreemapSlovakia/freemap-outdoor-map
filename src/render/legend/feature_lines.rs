use crate::render::{
    layers::Category,
    legend::{
        LegendItem,
        mapping::MappingEntry,
        shared::{leak_str, legend_feature_data_builder, with_landcover},
    },
};
use indexmap::IndexMap;
use std::collections::HashMap;

pub fn feature_lines(mapping_entries: &[MappingEntry]) -> Vec<LegendItem<'static>> {
    [
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
    ]
    .iter()
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

                    for entry in mapping_entries {
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
                        .with_line_string(zoom, false)
                        .build(),
                )
                .build(),
            zoom,
        )
    })
    .collect()
}
