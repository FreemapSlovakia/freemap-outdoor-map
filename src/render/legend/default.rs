use super::mapping;
use super::{LegendItem, mapping_path};
use crate::render::layers::Category;
use crate::render::legend::feature_lines::feature_lines;
use crate::render::legend::shared::{
    legend_feature_data_builder, legend_item_data_builder, polygon,
};
use crate::render::legend::{landcovers::landcovers, pois::pois, roads::roads};
use geo::Point;
use indexmap::IndexMap;
use mapping::collect_mapping_entries;
use std::io::BufReader;

pub(super) fn build_default_legend_items() -> Vec<LegendItem<'static>> {
    let mapping_root: mapping::MappingRoot = {
        let mapping_file = std::fs::File::open(mapping_path()).expect("read mapping.yaml");

        serde_saphyr::from_reader(BufReader::new(mapping_file)).expect("parse mapping.yaml")
    };

    let mapping_entries = collect_mapping_entries(&mapping_root);

    let poi_items = pois(&mapping_root, &mapping_entries);

    let landcover_items = landcovers(&mapping_entries);

    let roads = roads();

    let lines = feature_lines(&mapping_entries);

    let water = [
        &["river", "canal"] as &[&str],
        &[
            "stream",
            "ditch",
            "drain",
            "rapids",
            "tidal_channel",
            "pressurised",
            "canoe_pass",
            "fish_pass",
        ],
    ]
    .iter()
    .map(|types| {
        LegendItem::new(
            format!("river_{}", types[0]).leak(),
            Category::Water,
            types
                .iter()
                .map(|typ| IndexMap::from([("waterway", *typ)]))
                .collect::<Vec<_>>(),
            legend_item_data_builder()
                .with_feature(
                    "water_lines",
                    legend_feature_data_builder()
                        .with_line_string(17, false)
                        .with("name", "Abc")
                        .with("type", types[0])
                        .with("tmp", false)
                        .with("tunnel", false)
                        .build(),
                )
                .build(),
            17,
        )
    })
    .chain([
        LegendItem::new(
            "waterway_tmp",
            Category::Water,
            [
                [("waterway", "*"), ("intermittent", "yes")].into(),
                [("waterway", "*"), ("seasonal", "yes")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "water_lines",
                    legend_feature_data_builder()
                        .with_line_string(17, false)
                        .with("name", "Abc")
                        .with("type", "stream")
                        .with("tmp", true)
                        .with("tunnel", false)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "waterway_culvert",
            Category::Water,
            [[("tunnel", "culvert")].into()],
            legend_item_data_builder()
                .with_feature(
                    "water_lines",
                    legend_feature_data_builder()
                        .with_line_string(17, false)
                        .with("name", "Abc")
                        .with("type", "stream")
                        .with("tmp", false)
                        .with("tunnel", true)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "water_area",
            Category::Water,
            [[("natural", "water")].into()],
            legend_item_data_builder()
                .with_feature(
                    "water_areas",
                    legend_feature_data_builder()
                        .with("geometry", polygon(true, 17))
                        .with("name", "Abc")
                        .with("tmp", false)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "water_area_tmp",
            Category::Water,
            [
                [("natural", "water"), ("intermittent", "yes")].into(),
                [("natural", "water"), ("seasonal", "yes")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "water_areas",
                    legend_feature_data_builder()
                        .with("geometry", polygon(true, 17))
                        .with("name", "Abc")
                        .with("tmp", true)
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "solar_power_plants",
            Category::Landcover,
            [
                [("power", "plant"), ("plant:source", "solar")].into(),
                [("power", "generator"), ("generator:source", "solar")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "solar_power_plants",
                    legend_feature_data_builder()
                        .with("geometry", polygon(false, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "zoo",
            Category::Landcover,
            [
                [("tourism", "zoo")].into(),
                [("tourism", "theme_park")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "special_parks",
                    legend_feature_data_builder()
                        .with("geometry", polygon(true, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "country_borders",
            Category::Borders,
            [[
                ("type", "boundary"),
                ("boundary", "administrative"),
                ("admin_level", "2"),
            ]
            .into()],
            legend_item_data_builder()
                .with_feature(
                    "country_borders",
                    legend_feature_data_builder()
                        .with("geometry", polygon(true, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "military_areas",
            Category::Borders,
            [[("landuse", "military")].into()],
            legend_item_data_builder()
                .with_feature(
                    "military_areas",
                    legend_feature_data_builder()
                        .with("geometry", polygon(true, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "nature_reserve",
            Category::Borders,
            [
                [("leisure", "nature_reserve")].into(),
                [("boundary", "protected_area"), ("protect_class", "≠2")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "protected_areas",
                    legend_feature_data_builder()
                        .with("type", "nature_reserve")
                        .with("name", "Abc")
                        .with("protect_class", "")
                        .with("geometry", polygon(true, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "national_park",
            Category::Borders,
            [
                [("boundary", "national_park")].into(),
                [("boundary", "protected_area"), ("protect_class", "2")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "protected_areas",
                    legend_feature_data_builder()
                        .with("type", "national_park")
                        .with("name", "Abc")
                        .with("protect_class", "")
                        .with("geometry", polygon(true, 10))
                        .build(),
                )
                .build(),
            10,
        ),
        LegendItem::new(
            "national_park_zoom",
            Category::Borders,
            [
                [("boundary", "national_park")].into(),
                [("boundary", "protected_area"), ("protect_class", "2")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "protected_areas",
                    legend_feature_data_builder()
                        .with("type", "national_park")
                        .with("name", "")
                        .with("protect_class", "")
                        .with("geometry", polygon(true, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "building",
            Category::Other,
            [[("building", "*")].into()],
            legend_item_data_builder()
                .with_feature(
                    "buildings",
                    legend_feature_data_builder()
                        .with("type", "yes")
                        .with("geometry", polygon(false, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "building_disused",
            Category::Other,
            [
                [("building", "disused")].into(),
                [("building", "*"), ("disused", "yes")].into(),
                [("disused:building", "*")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "buildings",
                    legend_feature_data_builder()
                        .with("type", "disused")
                        .with("geometry", polygon(false, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "building_abandoned",
            Category::Other,
            [
                [("building", "abandoned")].into(),
                [("building", "*"), ("abandoned", "yes")].into(),
                [("abandoned:building", "*")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "buildings",
                    legend_feature_data_builder()
                        .with("type", "abandoned")
                        .with("geometry", polygon(false, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "building_ruins",
            Category::Other,
            [
                [("building", "ruins")].into(),
                [("building", "*"), ("ruins", "yes")].into(),
                [("ruins:building", "*")].into(),
            ],
            legend_item_data_builder()
                .with_feature(
                    "buildings",
                    legend_feature_data_builder()
                        .with("type", "ruins")
                        .with("geometry", polygon(false, 17))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "fixme",
            Category::Other,
            [[("fixme", "*")].into()],
            legend_item_data_builder()
                .with_feature(
                    "fixmes",
                    legend_feature_data_builder()
                        .with("geometry", Point::new(0.0, 0.0))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "simple_tree",
            Category::NaturalPoi,
            [[("natural", "tree")].into()],
            legend_item_data_builder()
                .with_feature(
                    "trees",
                    legend_feature_data_builder()
                        .with("type", "tree")
                        .with("geometry", Point::new(0.0, 0.0))
                        .build(),
                )
                .build(),
            17,
        ),
        LegendItem::new(
            "simple_shrub",
            Category::NaturalPoi,
            [[("natural", "shrub")].into()],
            legend_item_data_builder()
                .with_feature(
                    "trees",
                    legend_feature_data_builder()
                        .with("type", "shrub")
                        .with("geometry", Point::new(0.0, 0.0))
                        .build(),
                )
                .build(),
            17,
        ),
    ]);

    poi_items
        .into_iter()
        .chain(landcover_items)
        .chain(roads)
        .chain(lines)
        .chain(water)
        .collect()
}
