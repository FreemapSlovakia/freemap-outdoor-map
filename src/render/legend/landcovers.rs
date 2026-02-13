use crate::render::{
    layers::{Category, PAINT_DEFS},
    legend::{
        LegendItem, LegendItemData,
        mapping::{MappingEntry, MappingKind},
        shared::{
            build_tags_map, leak_str, legend_feature_data_builder, legend_item_data_builder,
            polygon,
        },
    },
};
use indexmap::IndexMap;
use std::collections::HashMap;

pub fn landcovers(mapping_entries: &[MappingEntry]) -> Vec<LegendItem<'static>> {
    let mut landcover_tags = HashMap::<&'static str, &'static str>::new();

    for entry in mapping_entries {
        if entry.table == "landcovers"
            && matches!(
                entry.kind,
                MappingKind::TableMapping | MappingKind::TableMappingNested
            )
        {
            landcover_tags.insert(leak_str(&entry.value), leak_str(&entry.key));
        }
    }

    PAINT_DEFS
        .iter()
        .map(|(types, _paints)| {
            let mut tags = Vec::with_capacity(types.len());

            for typ in *types {
                tags.push(build_landcover_tags(typ, &landcover_tags));
            }

            let id_typ = types[0];

            let skew = !matches!(id_typ, "silo" | "parking");

            LegendItem::new(
                format!("landcover_{id_typ}").leak(),
                Category::Landcover,
                tags,
                build_landcover_data(id_typ, skew, 19),
                19,
            )
        })
        .collect()
}

fn build_landcover_tags(
    typ: &'static str,
    landcover_tags: &HashMap<&'static str, &'static str>,
) -> IndexMap<&'static str, &'static str> {
    let mut tags = vec![];

    if let Some(value) = landcover_tags.get(typ) {
        tags.push((*value, typ));
    }

    if matches!(
        typ,
        "bog" | "reedbed" | "marsh" | "swamp" | "wet_meadow" | "mangrove" | "fen"
    ) {
        tags.push(("natural", "wetland"));
        tags.push(("wetland", typ));
    }

    build_tags_map(tags)
}

fn build_landcover_data(typ: &'static str, skew: bool, zoom: u8) -> LegendItemData {
    legend_item_data_builder()
        .with_feature(
            "landcovers",
            legend_feature_data_builder()
                .with("type", typ)
                .with("name", "Abc")
                .with("geometry", polygon(skew, zoom))
                .build(),
        )
        .build()
}
