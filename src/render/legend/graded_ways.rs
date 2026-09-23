use crate::render::{
    layers::Category,
    legend::{BuildOpts, LegendItem},
};

/// The graded-way overlays are gated at zoom 12 in `layers::pipeline`: below it the
/// generalized road tables have dropped the ways that carry the grade.
const GRADED_FROM_ZOOM: u8 = 12;

/// One item per step of an ordered way grade, drawn by the layer of the same name.
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
            LegendItem::builder(format!("{key}_{value}").leak(), Category::RoadsAndPaths, 17, opts)
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

pub fn graded_ways(opts: BuildOpts) -> Vec<LegendItem<'static>> {
    let mut items = graded(
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
        opts,
    );

    items.extend(graded(
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
        opts,
    ));

    items
}
