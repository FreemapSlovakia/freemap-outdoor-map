use super::mapping;
use super::{BuildOpts, LegendItem, mapping_path};
use crate::render::layers::Category;
use crate::render::legend::feature_lines::feature_lines;
use crate::render::legend::{landcovers::landcovers, pois::pois, roads::roads};
use geo::Point;
use mapping::collect_mapping_entries;
use std::io::BufReader;
use std::sync::LazyLock;

/// Parsed once: the items are rebuilt for every requested zoom, and re-reading the YAML
/// each time would dominate the cost.
static MAPPING: LazyLock<(mapping::MappingRoot, Vec<mapping::MappingEntry>)> =
    LazyLock::new(|| {
        let mapping_root: mapping::MappingRoot = {
            let mapping_file = std::fs::File::open(mapping_path()).expect("read mapping.yaml");

            serde_saphyr::from_reader(BufReader::new(mapping_file)).expect("parse mapping.yaml")
        };

        let mapping_entries = collect_mapping_entries(&mapping_root);

        (mapping_root, mapping_entries)
    });

/// Rivers and canals are drawn at every zoom, but `layers::water_lines::render` scales their
/// width by `1.5^(zoom - 8)`, which at zoom 0 leaves a stroke too thin to put down a pixel.
const RIVER_FROM_ZOOM: u8 = 1;

/// Every other waterway in `layers::water_lines::render` has an arm only from zoom 12.
const MINOR_WATERWAY_FROM_ZOOM: u8 = 12;

/// `layers::pipeline` gates both protected area stages at zoom 8; below that the hatch fill
/// and the border pattern are simply not drawn.
const PROTECTED_AREA_FROM_ZOOM: u8 = 8;

/// Gate for the `buildings` stage in `layers::pipeline`.
const BUILDING_FROM_ZOOM: u8 = 13;

pub(super) fn build_legend_items(opts: BuildOpts) -> Vec<LegendItem<'static>> {
    let (mapping_root, mapping_entries) = &*MAPPING;

    let poi_items = pois(mapping_root, mapping_entries, opts);

    let landcover_items = landcovers(mapping_entries, opts);

    let roads = roads(opts);

    let lines = feature_lines(mapping_entries, opts);

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
            opts,
        )
        .add_tag_set(|mut ts| {
            for typ in *types {
                ts = ts.add_tags(|tags| tags.add("waterway", typ));
            }
            ts
        })
        .min_zoom(if types[0] == "river" {
            RIVER_FROM_ZOOM
        } else {
            MINOR_WATERWAY_FROM_ZOOM
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
        LegendItem::builder("waterway_tmp", Category::Water, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("waterway", "*").add("intermittent", "yes"))
                    .add_tags(|tags| tags.add("waterway", "*").add("seasonal", "yes"))
            })
            .min_zoom(MINOR_WATERWAY_FROM_ZOOM)
            .add_feature("water_lines", |b| {
                b.with_line_string(false)
                    .with_name()
                    .with("type", "stream")
                    .with("tmp", true)
                    .with("tunnel", false)
            })
            .build(),
        LegendItem::builder("waterway_culvert", Category::Water, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("tunnel", "culvert")))
            .min_zoom(MINOR_WATERWAY_FROM_ZOOM)
            .add_feature("water_lines", |b| {
                b.with_line_string(false)
                    .with_name()
                    .with("type", "stream")
                    .with("tmp", false)
                    .with("tunnel", true)
            })
            .build(),
        LegendItem::builder("water_area", Category::Water, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("natural", "water"))
                    .add_tags(|tags| tags.add("landuse", "basin"))
                    .add_tags(|tags| tags.add("landuse", "reservoir"))
                    .add_tags(|tags| tags.add("amenity", "swimming_pool"))
                    .add_tags(|tags| tags.add("amenity", "fountain"))
                    .add_tags(|tags| tags.add("leisure", "swimming_pool"))
                    .add_tags(|tags| tags.add("natural", "water"))
                    .add_tags(|tags| tags.add("waterway", "waterway"))
            })
            .add_feature("water_areas", |b| {
                b.with_polygon(true).with_name().with("tmp", false)
            })
            .build(),
        LegendItem::builder("water_area_tmp", Category::Water, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("natural", "water").add("intermittent", "yes"))
                    .add_tags(|tags| tags.add("natural", "water").add("seasonal", "yes"))
            })
            .add_feature("water_areas", |b| {
                b.with_polygon(true).with_name().with("tmp", true)
            })
            .build(),
        LegendItem::builder("solar_power_plants", Category::ManMade, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("power", "plant").add("plant:source", "solar"))
                    .add_tags(|tags| {
                        tags.add("power", "generator")
                            .add("generator:source", "solar")
                    })
            })
            .min_zoom(12)
            .add_feature("solar_power_plants", |b| b.with_polygon(false))
            .build(),
        LegendItem::builder("zoo", Category::Tourism, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("tourism", "zoo"))
                    .add_tags(|tags| tags.add("tourism", "theme_park"))
            })
            .min_zoom(13)
            .add_feature("special_parks", |b| b.with_polygon(true))
            .build(),
        LegendItem::builder("country_borders", Category::Borders, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| {
                    tags.add("type", "boundary")
                        .add("boundary", "administrative")
                        .add("admin_level", "2")
                })
            })
            // The low zoom stage that also draws borders is behind `RenderLayer::CountryNames`,
            // which legend renders do not ask for.
            .min_zoom(8)
            .add_feature("country_borders", |b| b.with_polygon(true))
            .build(),
        LegendItem::builder("military_areas", Category::Borders, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("landuse", "military")))
            .min_zoom(10)
            .add_feature("military_areas", |b| b.with_polygon(true))
            .build(),
        LegendItem::builder("nature_reserve", Category::Borders, 15, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("leisure", "nature_reserve"))
                    .add_tags(|tags| {
                        tags.add("boundary", "protected_area")
                            .add("protect_class", "≠2")
                    })
            })
            .min_zoom(PROTECTED_AREA_FROM_ZOOM)
            .add_feature("protected_areas", |b| {
                b.with("type", "nature_reserve")
                    .with_name()
                    .with("protect_class", "")
                    .with_polygon(true)
            })
            .build(),
        LegendItem::builder("national_park", Category::Borders, 10, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("boundary", "national_park"))
                    .add_tags(|tags| {
                        tags.add("boundary", "protected_area")
                            .add("protect_class", "2")
                    })
            })
            .min_zoom(PROTECTED_AREA_FROM_ZOOM)
            .add_feature("protected_areas", |b| {
                b.with("type", "national_park")
                    .with_name()
                    .with("protect_class", "")
                    .with_polygon(true)
            })
            .build(),
        LegendItem::builder("building", Category::Other, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("building", "*")))
            .min_zoom(BUILDING_FROM_ZOOM)
            .add_feature("buildings", |b| b.with("type", "yes").with_polygon(false))
            .build(),
        LegendItem::builder("building_disused", Category::Other, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("building", "disused"))
                    .add_tags(|tags| tags.add("building", "*").add("disused", "yes"))
                    .add_tags(|tags| tags.add("disused:building", "*"))
            })
            .min_zoom(BUILDING_FROM_ZOOM)
            .add_feature("buildings", |b| {
                b.with("type", "disused").with_polygon(false)
            })
            .build(),
        LegendItem::builder("building_abandoned", Category::Other, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("building", "abandoned"))
                    .add_tags(|tags| tags.add("building", "*").add("abandoned", "yes"))
                    .add_tags(|tags| tags.add("abandoned:building", "*"))
            })
            .min_zoom(BUILDING_FROM_ZOOM)
            .add_feature("buildings", |b| {
                b.with("type", "abandoned").with_polygon(false)
            })
            .build(),
        LegendItem::builder("building_ruins", Category::Other, 17, opts)
            .add_tag_set(|ts| {
                ts.add_tags(|tags| tags.add("building", "ruins"))
                    .add_tags(|tags| tags.add("building", "*").add("ruins", "yes"))
                    .add_tags(|tags| tags.add("ruins:building", "*"))
            })
            .min_zoom(BUILDING_FROM_ZOOM)
            .add_feature("buildings", |b| b.with("type", "ruins").with_polygon(false))
            .build(),
        LegendItem::builder("fixme", Category::Other, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("fixme", "*")))
            .min_zoom(14)
            .add_feature("fixmes", |b| b.with("geometry", Point::new(0.0, 0.0)))
            .build(),
        LegendItem::builder("simple_tree", Category::NaturalPoi, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("natural", "tree")))
            .min_zoom(16)
            .add_feature("trees", |b| {
                b.with("type", "tree")
                    .with("geometry", Point::new(0.0, 0.0))
            })
            .build(),
        LegendItem::builder("simple_shrub", Category::NaturalPoi, 17, opts)
            .add_tag_set(|ts| ts.add_tags(|tags| tags.add("natural", "shrub")))
            .min_zoom(16)
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
