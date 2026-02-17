use super::mapping;
use super::{LegendItem, mapping_path};
use crate::render::layers::Category;
use crate::render::legend::feature_lines::feature_lines;
use crate::render::legend::{landcovers::landcovers, pois::pois, roads::roads};
use geo::Point;
use mapping::collect_mapping_entries;
use std::io::BufReader;

pub(super) fn build_legend_items(for_taginfo: bool) -> Vec<LegendItem<'static>> {
    let mapping_root: mapping::MappingRoot = {
        let mapping_file = std::fs::File::open(mapping_path()).expect("read mapping.yaml");

        serde_saphyr::from_reader(BufReader::new(mapping_file)).expect("parse mapping.yaml")
    };

    let mapping_entries = collect_mapping_entries(&mapping_root);

    let poi_items = pois(&mapping_root, &mapping_entries, for_taginfo);

    let landcover_items = landcovers(&mapping_entries, for_taginfo);

    let roads = roads(for_taginfo);

    let lines = feature_lines(&mapping_entries, for_taginfo);

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
        LegendItem::builder(
            format!("river_{}", types[0]).leak(),
            Category::Water,
            17,
            for_taginfo,
        )
        .add_tag_set(|mut ts| {
            for typ in types.iter() {
                ts = ts.add_tags(|tags| tags.add("waterway", typ));
            }
            ts
        })
        .add_feature("water_lines", |b| {
            b.with_line_string(false)
                .with_name()
                .with("type", types[0])
                .with("tmp", false)
                .with("tunnel", false)
        })
        .build()
    })
    .chain([
        LegendItem::builder("waterway_tmp", Category::Water, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("waterway", "*").add("intermittent", "yes"))
                    .add_tags(|tags| tags.add("waterway", "*").add("seasonal", "yes"))
            })
            .add_feature("water_lines", |b| {
                b.with_line_string(false)
                    .with_name()
                    .with("type", "stream")
                    .with("tmp", true)
                    .with("tunnel", false)
            })
            .build(),
        LegendItem::builder("waterway_culvert", Category::Water, 17, for_taginfo)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("tunnel", "culvert")))
            .add_feature("water_lines", |b| {
                b.with_line_string(false)
                    .with_name()
                    .with("type", "stream")
                    .with("tmp", false)
                    .with("tunnel", true)
            })
            .build(),
        LegendItem::builder("water_area", Category::Water, 17, for_taginfo)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("natural", "water")))
            .add_feature("water_areas", |b| {
                b.with_polygon(true).with_name().with("tmp", false)
            })
            .build(),
        LegendItem::builder("water_area_tmp", Category::Water, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("natural", "water").add("intermittent", "yes"))
                    .add_tags(|tags| tags.add("natural", "water").add("seasonal", "yes"))
            })
            .add_feature("water_areas", |b| {
                b.with_polygon(true).with_name().with("tmp", true)
            })
            .build(),
        LegendItem::builder("solar_power_plants", Category::Landcover, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("power", "plant").add("plant:source", "solar"))
                    .add_tags(|tags| {
                        tags.add("power", "generator")
                            .add("generator:source", "solar")
                    })
            })
            .add_feature("solar_power_plants", |b| b.with_polygon(false))
            .build(),
        LegendItem::builder("zoo", Category::Landcover, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("tourism", "zoo"))
                    .add_tags(|tags| tags.add("tourism", "theme_park"))
            })
            .add_feature("special_parks", |b| b.with_polygon(true))
            .build(),
        LegendItem::builder("country_borders", Category::Borders, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| {
                    tags.add("type", "boundary")
                        .add("boundary", "administrative")
                        .add("admin_level", "2")
                })
            })
            .add_feature("country_borders", |b| b.with_polygon(true))
            .build(),
        LegendItem::builder("military_areas", Category::Borders, 17, for_taginfo)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("landuse", "military")))
            .add_feature("military_areas", |b| b.with_polygon(true))
            .build(),
        LegendItem::builder("nature_reserve", Category::Borders, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("leisure", "nature_reserve"))
                    .add_tags(|tags| {
                        tags.add("boundary", "protected_area")
                            .add("protect_class", "≠2")
                    })
            })
            .add_feature("protected_areas", |b| {
                b.with("type", "nature_reserve")
                    .with_name()
                    .with("protect_class", "")
                    .with_polygon(true)
            })
            .build(),
        LegendItem::builder("national_park", Category::Borders, 10, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("boundary", "national_park"))
                    .add_tags(|tags| {
                        tags.add("boundary", "protected_area")
                            .add("protect_class", "2")
                    })
            })
            .add_feature("protected_areas", |b| {
                b.with("type", "national_park")
                    .with_name()
                    .with("protect_class", "")
                    .with_polygon(true)
            })
            .build(),
        LegendItem::builder("national_park_zoom", Category::Borders, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("boundary", "national_park"))
                    .add_tags(|tags| {
                        tags.add("boundary", "protected_area")
                            .add("protect_class", "2")
                    })
            })
            .add_feature("protected_areas", |b| {
                b.with("type", "national_park")
                    .with("name", "")
                    .with("protect_class", "")
                    .with_polygon(true)
            })
            .build(),
        LegendItem::builder("building", Category::Other, 17, for_taginfo)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("building", "*")))
            .add_feature("buildings", |b| b.with("type", "yes").with_polygon(false))
            .build(),
        LegendItem::builder("building_disused", Category::Other, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("building", "disused"))
                    .add_tags(|tags| tags.add("building", "*").add("disused", "yes"))
                    .add_tags(|tags| tags.add("disused:building", "*"))
            })
            .add_feature("buildings", |b| {
                b.with("type", "disused").with_polygon(false)
            })
            .build(),
        LegendItem::builder("building_abandoned", Category::Other, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("building", "abandoned"))
                    .add_tags(|tags| tags.add("building", "*").add("abandoned", "yes"))
                    .add_tags(|tags| tags.add("abandoned:building", "*"))
            })
            .add_feature("buildings", |b| {
                b.with("type", "abandoned").with_polygon(false)
            })
            .build(),
        LegendItem::builder("building_ruins", Category::Other, 17, for_taginfo)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("building", "ruins"))
                    .add_tags(|tags| tags.add("building", "*").add("ruins", "yes"))
                    .add_tags(|tags| tags.add("ruins:building", "*"))
            })
            .add_feature("buildings", |b| b.with("type", "ruins").with_polygon(false))
            .build(),
        LegendItem::builder("fixme", Category::Other, 17, for_taginfo)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("fixme", "*")))
            .add_feature("fixmes", |b| b.with("geometry", Point::new(0.0, 0.0)))
            .build(),
        LegendItem::builder("simple_tree", Category::NaturalPoi, 17, for_taginfo)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("natural", "tree")))
            .add_feature("trees", |b| {
                b.with("type", "tree")
                    .with("geometry", Point::new(0.0, 0.0))
            })
            .build(),
        LegendItem::builder("simple_shrub", Category::NaturalPoi, 17, for_taginfo)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("natural", "shrub")))
            .add_feature("trees", |b| {
                b.with("type", "shrub")
                    .with("geometry", Point::new(0.0, 0.0))
            })
            .build(),
    ]);

    poi_items
        .into_iter()
        .chain(landcover_items)
        .chain(roads)
        .chain(lines)
        .chain(water)
        .collect()
}
