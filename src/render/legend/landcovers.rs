use crate::render::{
    layers::{Category, PAINT_DEFS},
    legend::{
        LegendItem, build_tags_map, leak_str,
        mapping::{MappingEntry, MappingKind},
    },
};
use indexmap::IndexMap;
use std::collections::HashMap;

pub fn landcovers(mapping_entries: &[MappingEntry], for_taginfo: bool) -> Vec<LegendItem<'static>> {
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

            LegendItem::builder(
                format!("landcover_{id_typ}").leak(),
                Category::Landcover,
                19,
                for_taginfo,
            )
            .add_tag_set(|mut ts| {
                for tag_set in &tags {
                    ts = ts.add_tags(|mut tb| {
                        for (k, v) in tag_set {
                            tb = tb.add(k, v);
                        }
                        tb
                    });
                }
                ts
            })
            .add_feature("landcovers", |b| {
                b.with("type", id_typ).with_name().with_polygon(skew)
            })
            .build()
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
