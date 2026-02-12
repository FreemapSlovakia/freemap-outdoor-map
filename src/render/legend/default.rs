use super::mapping;
use super::{LegendItem, mapping_path};
use crate::render::legend::feature_lines::feature_lines;
use crate::render::legend::{landcovers::landcovers, pois::pois, roads::roads};
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

    poi_items
        .into_iter()
        .chain(landcover_items)
        .chain(roads)
        .chain(lines)
        .collect()
}
