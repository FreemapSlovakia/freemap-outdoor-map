use crate::render::{
    layers::Category,
    legend::{BuildOpts, LegendItem},
};

/// Unlike the other legend files, this one can import the zoom it restates: the
/// graded overlays live in `draw::graded_dots`, which is public.
use crate::render::draw::graded_dots::MIN_ZOOM as GRADED_FROM_ZOOM;

/// One item per step of an ordered way grade. The id follows the layer, the tag
/// follows the key — they differ where the OSM key is not a legal column name.
/// The enumerate is 1-based in the order the values appear in `mapping.yaml`, and
/// the legend has to agree with it or the samples come out the wrong colour.
fn graded(
    layer: &'static str,
    key: &'static str,
    values: &[&'static str],
    opts: BuildOpts,
) -> Vec<LegendItem<'static>> {
    values
        .iter()
        .enumerate()
        .map(|(i, value)| {
            LegendItem::builder(format!("{layer}_{value}").leak(), Category::RoadsAndPaths, 17, opts)
                .add_tag_set(|ts| ts.add_tags(|tags| tags.add(key, value)))
                .min_zoom(GRADED_FROM_ZOOM)
                .add_landcover("wood")
                .add_feature(layer, |b| {
                    b.with_line_string(false).with("grade", i as i32 + 1)
                })
                .build()
        })
        .collect()
}

/// Every graded overlay's legend: the layer it draws into, the OSM key its
/// samples are tagged with, and the value for each step — in the order the
/// colour ramp indexes them.
const GRADES: [(&str, &str, &[&str]); 5] = [
    (
        "sac_scale",
        "sac_scale",
        &[
            "hiking",
            "mountain_hiking",
            "demanding_mountain_hiking",
            "alpine_hiking",
            "demanding_alpine_hiking",
            "difficult_alpine_hiking",
        ],
    ),
    (
        "smoothness",
        "smoothness",
        &[
            "excellent",
            "good",
            "intermediate",
            "bad",
            "very_bad",
            "horrible",
            "very_horrible",
            "impassable",
        ],
    ),
    ("mtb_scale", "mtb:scale", &["0", "1", "2", "3", "4", "5", "6"]),
    (
        "piste_difficulty",
        "piste:difficulty",
        &[
            "novice",
            "easy",
            "intermediate",
            "advanced",
            "expert",
            "freeride",
            "extreme",
        ],
    ),
    (
        "via_ferrata_scale",
        "via_ferrata_scale",
        &["0", "1", "2", "3", "4", "5", "6"],
    ),
];

pub fn graded_ways(opts: BuildOpts) -> Vec<LegendItem<'static>> {
    GRADES
        .iter()
        .flat_map(|(layer, key, values)| graded(layer, key, values, opts))
        .collect()
}
