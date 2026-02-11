use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub(crate) struct MappingRoot {
    #[serde(default)]
    pub(crate) tables: HashMap<String, Table>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Table {
    #[serde(default)]
    pub(crate) mapping: Option<MappingValues>,
    #[serde(default)]
    pub(crate) mappings: Option<HashMap<String, SubMapping>>,
    #[serde(default)]
    pub(crate) columns: Option<Vec<Column>>,
    #[serde(default)]
    pub(crate) type_mappings: Option<TypeMappings>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Column {
    #[serde(rename = "type")]
    pub(crate) column_type: String,
    #[serde(default)]
    pub(crate) aliases: Option<HashMap<String, HashMap<String, String>>>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TypeMappings {
    #[serde(default)]
    pub(crate) points: Option<TypeMapping>,
    #[serde(default)]
    pub(crate) linestrings: Option<TypeMapping>,
    #[serde(default)]
    pub(crate) polygons: Option<TypeMapping>,
    #[serde(default)]
    pub(crate) any: Option<TypeMapping>,
}

pub(crate) type MappingValues = HashMap<String, Vec<String>>;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum TypeMapping {
    Direct(MappingValues),
    Expanded(ExpandedTypeMapping),
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExpandedTypeMapping {
    #[serde(default)]
    pub(crate) mapping: Option<MappingValues>,
    #[serde(default)]
    pub(crate) mappings: Option<HashMap<String, SubMapping>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubMapping {
    pub(crate) mapping: MappingValues,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MappingKind {
    TableMapping,
    TableMappingNested,
    TypeMappingDirect,
    TypeMappingNested,
}

#[derive(Debug)]
pub(crate) struct MappingEntry {
    pub(crate) table: String,
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) kind: MappingKind,
}

pub(crate) fn collect_mapping_entries(root: &MappingRoot) -> Vec<MappingEntry> {
    let mut entries = Vec::new();

    for (table_name, table) in &root.tables {
        if let Some(mapping) = &table.mapping {
            push_mapping_entries(&mut entries, table_name, mapping, MappingKind::TableMapping);
        }
        if let Some(mappings) = &table.mappings {
            for sub_mapping in mappings.values() {
                push_mapping_entries(
                    &mut entries,
                    table_name,
                    &sub_mapping.mapping,
                    MappingKind::TableMappingNested,
                );
            }
        }

        let Some(type_mappings) = &table.type_mappings else {
            continue;
        };

        for tm_opt in [
            &type_mappings.points,
            &type_mappings.linestrings,
            &type_mappings.polygons,
            &type_mappings.any,
        ] {
            let Some(tm) = tm_opt else {
                continue;
            };

            match tm {
                TypeMapping::Direct(mapping) => {
                    push_mapping_entries(
                        &mut entries,
                        table_name,
                        mapping,
                        MappingKind::TypeMappingDirect,
                    );
                }
                TypeMapping::Expanded(expanded) => {
                    if let Some(mapping) = &expanded.mapping {
                        push_mapping_entries(
                            &mut entries,
                            table_name,
                            mapping,
                            MappingKind::TypeMappingDirect,
                        );
                    }
                    if let Some(mappings) = &expanded.mappings {
                        for sub_mapping in mappings.values() {
                            push_mapping_entries(
                                &mut entries,
                                table_name,
                                &sub_mapping.mapping,
                                MappingKind::TypeMappingNested,
                            );
                        }
                    }
                }
            }
        }
    }

    entries
}

fn push_mapping_entries(
    entries: &mut Vec<MappingEntry>,
    table: &str,
    mapping: &MappingValues,
    kind: MappingKind,
) {
    for (key, values) in mapping {
        for value in values {
            entries.push(MappingEntry {
                table: table.to_string(),
                key: key.to_string(),
                value: value.to_string(),
                kind,
            });
        }
    }
}
