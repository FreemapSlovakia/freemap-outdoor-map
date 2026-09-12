use crate::render::{
    layers::{Category, Def, POI_ORDER, POIS, SHOP_TYPES},
    legend::{
        BuildOpts, LegendItem, LegendItemBuilder, MAX_LEGEND_ZOOM, build_tags_map, leak_str,
        mapping::{self, MappingEntry},
    },
};
use geo::Point;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

/// Sample zoom when the legend is requested without one.
const PREFERRED_POI_ZOOM: u8 = 19;

pub fn pois(
    mapping_root: &mapping::MappingRoot,
    mapping_entries: &[MappingEntry],
    opts: BuildOpts,
) -> Vec<LegendItem<'static>> {
    let mut poi_tags: HashMap<&'static str, Vec<(&'static str, &'static str)>> = HashMap::new();
    let mut feature_alias_values: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    let mut feature_alias_catchall: HashSet<&'static str> = HashSet::new();

    if let Some(pois) = mapping_root.tables.get("pois")
        && let Some(columns) = &pois.columns
    {
        for column in columns {
            if column.column_type != "mapping_value" {
                continue;
            }

            let Some(aliases) = &column.aliases else {
                continue;
            };

            for (key, values) in aliases {
                let key = leak_str(key);

                for (value, alias) in values {
                    let value = leak_str(value);
                    let alias = leak_str(alias);

                    if value == "__any__" {
                        feature_alias_catchall.insert(key);
                        poi_tags.entry(alias).or_default().push((key, "yes")); // "*"
                        continue;
                    }

                    feature_alias_values.entry(key).or_default().insert(value);

                    // Several values can alias to the same icon (e.g. both
                    // `obstacle=vegetation` and `obstacle=dense_vegetation` →
                    // `obstacle_vegetation`). They share the same tag key, so keep only
                    // the first one listed (the `aliases` IndexMap preserves YAML order)
                    // rather than letting a later value silently overwrite it.
                    let entry = poi_tags.entry(alias).or_default();

                    if !entry.iter().any(|(k, _)| *k == key) {
                        entry.push((key, value));
                    }
                }
            }
        }
    }

    for entry in mapping_entries {
        if entry.table != "pois" && entry.table != "sports"
            || feature_alias_catchall.contains(entry.key.as_str())
            || feature_alias_values
                .get(entry.key.as_str())
                .is_some_and(|values| values.contains(entry.value.as_str()))
        {
            continue;
        }

        let value = leak_str(&entry.value);
        let key = leak_str(&entry.key);

        poi_tags.entry(value).or_default().push((key, value));
    }

    struct PoiGroup {
        category: Category,
        tags: Vec<IndexMap<&'static str, &'static str>>,
        repr_typ: &'static str,
        zooms: RangeInclusive<u8>,
    }

    let mut poi_groups: IndexMap<&'static str, PoiGroup> = IndexMap::new();

    for typ in POI_ORDER.iter() {
        // `*_noname` types are the same feature as their named counterpart, drawn with
        // the same icon - listing them again would only repeat the entry with no tags
        // of their own to show.
        if typ.ends_with("_noname") || typ.starts_with("peak") && typ.len() == 5 {
            continue;
        }

        let Some(defs) = POIS.get(*typ) else {
            continue;
        };

        // Deepest zoom, so item ids stay the same at every zoom and a type drawn only at
        // MAX_LEGEND_ZOOM (the lowest-priority shops) still matches a definition.
        let Some(def) = defs.iter().find(|def| def.is_active_at(MAX_LEGEND_ZOOM)) else {
            continue;
        };

        let visual_key = if *typ == "volcano" {
            typ
        } else {
            def.icon_key(typ)
        };

        let zooms = poi_zooms(defs);

        let entry = poi_groups.entry(visual_key).or_insert_with(|| PoiGroup {
            category: def.category,
            tags: Vec::new(),
            repr_typ: typ,
            zooms: zooms.clone(),
        });

        entry.tags.push(build_poi_tags(typ, &poi_tags));

        // The group's zoom range is the union over its members, so the member standing for
        // it has to be one the renderer actually draws at the start of that range - not
        // whichever happens to rank highest. `swimming` shares the `water_park` icon but
        // only from z16, while the group starts at z14.
        if *zooms.start() < *entry.zooms.start() {
            entry.repr_typ = typ;
        }

        entry.zooms =
            (*entry.zooms.start()).min(*zooms.start())..=(*entry.zooms.end()).max(*zooms.end());
    }

    // Read the legend in the order the map reveals things: earliest zoom first, and within
    // a zoom the collision priority, which is a fair importance ranking. Priority alone
    // would look arbitrary here - a legend item shows its zoom range, so zoom is the one
    // ordering a reader can see the reason for.
    poi_groups.sort_by_cached_key(|_, group| {
        let rank = POI_ORDER
            .iter()
            .position(|typ| *typ == group.repr_typ)
            .unwrap_or(usize::MAX);

        (*group.zooms.start(), rank)
    });

    poi_groups
        .into_iter()
        .map(|(visual_key, group)| {
            let PoiGroup {
                category,
                tags,
                repr_typ,
                zooms,
            } = group;

            // Must land inside the item's own range or the swatch renders empty, as the
            // deepest tier of shops would. Clamping leaves every other item where it was.
            let preferred_zoom = PREFERRED_POI_ZOOM.clamp(*zooms.start(), *zooms.end());

            LegendItem::builder(
                format!("poi_{visual_key}").leak(),
                category,
                preferred_zoom,
                opts,
            )
            .zoom_range(*zooms.start(), *zooms.end())
            .add_tag_set(|mut ts| {
                for tag_set in &tags {
                    ts = ts.add_tags(|mut tb| {
                        for (k, v) in tag_set {
                            tb = tb.add(k, v);
                        }
                        tb
                    });
                }

                if visual_key == "spring" {
                    ts = ts
                        .add_tags(|t| t.add("natural", "geyser"))
                        .add_tags(|t| t.add("man_made", "spring_box"));
                }

                ts
            })
            .add_poi(repr_typ, HashMap::new(), category)
            .build()
        })
        .chain(
            [
                (("drinkable", "yes"), ("drinking_water", "yes")),
                (("drinkable", "no"), ("drinking_water", "no")),
                (("hot", "true"), ("natural", "hot_spring")),
                (
                    ("water_characteristic", "mineral"),
                    ("water_characteristic", "mineral"),
                ),
                (("refitted", "yes"), ("refitted", "yes")),
                (("intermittent", "yes"), ("intermittent", "yes")),
            ]
            .map(|((prop_name, prop_value), (tag_key, tag_value))| {
                LegendItem::builder(
                    format!("poi_spring_{tag_key}_{tag_value}").leak(),
                    Category::Water,
                    PREFERRED_POI_ZOOM,
                    opts,
                )
                .add_tag_set(|mut ts| {
                    ts = ts.add_tags(|tags| {
                        tags.add(
                            "natural",
                            if tag_value == "hot_spring" {
                                "hot_spring"
                            } else {
                                "spring"
                            },
                        )
                        .add(tag_key, tag_value)
                    });

                    if prop_name == "intermittent" {
                        ts = ts
                            .add_tags(|tags| tags.add("natural", "spring").add("seasonal", "yes"));
                    }

                    ts
                })
                .zoom_range_of(poi_zooms_of("spring"))
                .add_poi(
                    "spring",
                    HashMap::<String, Option<String>>::from([(
                        prop_name.to_string(),
                        Some(prop_value.to_string()),
                    )]),
                    Category::Water,
                )
                .build()
            }),
        )
        .chain([{
            LegendItem::builder("private_poi", Category::Other, 19, opts)
                .add_tag_set(|ts| {
                    ts.add_tags(|tags| tags.add("access", "private"))
                        .add_tags(|tags| tags.add("access", "no"))
                })
                .zoom_range_of(poi_zooms_of("picnic_shelter"))
                .add_poi(
                    "picnic_shelter",
                    HashMap::<String, Option<String>>::from([(
                        "access".into(),
                        Some("private".into()),
                    )]),
                    Category::Other,
                )
                .build()
        }])
        .chain([{
            // `ruins=*` takes over the icon of whatever POI carries it, keeping that
            // POI's own zooms and label. Drawn on a chalet because a bare `ruins=yes`
            // has no primary tag, so it is never imported and renders nothing on its
            // own - `historic=ruins`, the plain ruins entry, covers that spelling. The
            // chalet is only the carrier, so it stays out of the tags: the entry is
            // about the modifier, whatever it is attached to, the same way `private_poi`
            // above lists `access` alone.
            LegendItem::builder("ruins_poi", Category::Other, 19, opts)
                .add_tag_set(|ts| ts.add_tags(|tags| tags.add("ruins", "yes")))
                .zoom_range_of(poi_zooms_of("chalet"))
                .add_poi(
                    "chalet",
                    HashMap::<String, Option<String>>::from([(
                        "ruins".into(),
                        Some("yes".into()),
                    )]),
                    Category::Other,
                )
                .build()
        }])
        .collect()
}

/// Zooms at which the POI layer draws any of these definitions, straight from the renderer's
/// own table, narrowed by the zoom the `poi_icons` stage is gated at in `layers::pipeline`.
fn poi_zooms(defs: &[Def]) -> RangeInclusive<u8> {
    const POI_ICONS_FROM_ZOOM: u8 = 10;

    let (min_zoom, max_zoom) = defs
        .iter()
        .map(Def::zoom_span)
        .fold((u8::MAX, 0), |(min_zoom, max_zoom), (from, to)| {
            (min_zoom.min(from), max_zoom.max(to))
        });

    min_zoom.max(POI_ICONS_FROM_ZOOM)..=max_zoom.min(MAX_LEGEND_ZOOM)
}

/// [`poi_zooms`] for a single POI type.
fn poi_zooms_of(typ: &str) -> RangeInclusive<u8> {
    POIS.get(typ)
        .map_or(0..=MAX_LEGEND_ZOOM, |defs| poi_zooms(defs))
}

fn build_poi_tags(
    typ: &'static str,
    poi_tags: &HashMap<&'static str, Vec<(&'static str, &'static str)>>,
) -> IndexMap<&'static str, &'static str> {
    let mut tags = vec![];

    // `mapping.yaml` maps `shop: __any__`, so these types carry no tag of their own for
    // `mapping_entries` to find and the legend would show a nameless row.
    if SHOP_TYPES.contains(&typ) {
        tags.push(("shop", typ));
    } else if matches!(
        typ,
        "shopping_cart"
            | "lean_to"
            | "public_transport"
            | "picnic_shelter"
            | "basic_hut"
            | "weather_shelter"
    ) {
        tags.push(("amenity", "shelter"));
        tags.push(("shelter_type", typ));
    } else {
        let mut override_key = None;

        match typ {
            s if typ.starts_with("tower_") || typ.starts_with("mast_") => {
                let (a, b) = s.split_once('_').expect("matched a name containing '_'");
                tags.push(("man_made", a));
                tags.push(("tower:type", b));
            }
            "tree_protected" => {
                override_key = Some("tree");
                tags.push(("protected", "yes"));
            }
            "tree" => {
                tags.push(("denotation", "natural_monument"));
            }
            "generator_wind" => {
                tags.push(("power", "generator"));
                tags.push(("generator:source", "wind")); // OR method = 'wind_turbine'
            }
            // These reach the map only through `osm_place_of_worships`, whose `building`
            // column is a plain string, not a `mapping_value` - so unlike most types they
            // have no entry in mapping.yaml to fall through to, and a missing arm here
            // leaves the item with no tags at all.
            "church" | "chapel" | "synagogue" | "mosque" | "cathedral" | "temple" => {
                tags.push(("building", typ));
            }
            "disused_mine" | "disused_adit" | "disused_mineshaft" => {
                override_key = Some(&typ[8..]);
                tags.push(("disused", "yes"));
            }
            // As above. Their `abandoned:man_made=*` alias does have a mapping entry, but no
            // reader can be expected to recognise the lifecycle prefix, so say it the same
            // way the disused pair does.
            "abandoned_adit" | "abandoned_mineshaft" => {
                override_key = Some(&typ[10..]);
                tags.push(("abandoned", "yes"));
            }
            // The generic obstacle icon is the `obstacle=*` catch-all (every
            // `obstacle_%` value except tree/vegetation collapses to `obstacle` in the
            // query), so represent it with the wildcard rather than a single value.
            "obstacle" => {
                tags.push(("obstacle", "*"));
            }
            _ => {}
        }

        if let Some(pairs) = poi_tags.get(override_key.unwrap_or(typ)) {
            for (key, value) in pairs {
                if *key == "information" {
                    tags.push(("tourism", key));
                }

                tags.push((key, value));
            }
        }
    }

    build_tags_map(tags)
}

impl LegendItemBuilder<'_> {
    fn add_poi(
        self,
        typ: &'static str,
        extra: HashMap<String, Option<String>>,
        category: Category,
    ) -> Self {
        let factor = (19.0 - self.zoom as f64).exp2();

        // Explicit per-category lookup table; clearer than merged arms.
        #[allow(clippy::match_same_arms)]
        let bg = match category {
            Category::RoadsAndPaths => "meadow",
            Category::Railway => "residential",
            Category::Transport => "residential",
            Category::Water => "meadow",
            Category::Terrain => "wood",
            Category::NaturalPoi => "wood",
            Category::Landcover => "",
            Category::Borders => "",
            Category::Accommodation => "residential",
            Category::GastroPoi => "commercial",
            Category::Shop => "commercial",
            Category::Sport => "pitch",
            Category::Tourism => "meadow",
            Category::Culture => "residential",
            Category::Historic => "residential",
            Category::Religion => "residential",
            Category::Institution => "residential",
            Category::Finance => "residential",
            Category::Health => "residential",
            Category::ManMade => "residential",
            Category::Barrier => "meadow",
            Category::Facility => "residential",
            Category::Other => "residential",
        };

        let offset = if self.for_taginfo { 0.0 } else { factor * -2.0 };

        self.add_landcover(bg).add_feature("pois", |b| {
            b.with("type", typ)
                .with_name()
                .with("extra", extra)
                .with("geometry", Point::new(0.0, offset))
        })
    }
}
