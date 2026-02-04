pub(crate) const LANDCOVER_Z_ORDER: &[&str] = &[
    "winter_sports",
    "pedestrian",
    "footway",
    "pitch",
    "library",
    "barracks",
    "parking",
    "cemetery",
    "grave_yard",
    "place_of_worship",
    "dam",
    "weir",
    "clearcut",
    "wetland",
    "scrub",
    "orchard",
    "vineyard",
    "railway",
    "landfill",
    "scree",
    "blockfield",
    "quarry",
    "park",
    "dog_park",
    "garden",
    "allotments",
    "village_green",
    "grass",
    "recreation_ground",
    "fell",
    "bare_rock",
    "heath",
    "meadow",
    "wood",
    "forest",
    "golf_course",
    "grassland",
    "farm",
    "zoo",
    "farmyard",
    "hospital",
    "kindergarten",
    "school",
    "college",
    "university",
    "retail",
    "commercial",
    "industrial",
    "farmland",
    "residential",
    "glacier",
];

pub(crate) fn build_landcover_z_order_case(column: &str) -> String {
    let mut case = format!("CASE {column}");

    for (idx, typ) in LANDCOVER_Z_ORDER.iter().enumerate() {
        case.push_str(&format!(" WHEN '{typ}' THEN {idx}"));
    }

    case.push_str(" END");

    case
}
