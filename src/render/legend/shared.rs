use crate::render::{LegendMode, LegendValue};
use geo::{Coord, LineString, Polygon};
use indexmap::IndexMap;
use std::collections::HashMap;

pub(crate) type LegendItemData = HashMap<String, Vec<HashMap<String, LegendValue>>>;
pub(super) type LegendFeatureData = HashMap<String, LegendValue>;

#[derive(Default)]
pub(super) struct LegendFeatureDataBuilder(pub(super) LegendFeatureData);

impl LegendFeatureDataBuilder {
    pub(super) fn with(mut self, key: impl Into<String>, value: impl Into<LegendValue>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    pub(super) fn with_line_string(self, zoom: u8, reverse: bool) -> Self {
        let factor = (17.0 - zoom as f64).exp2();

        let mut coords = vec![
            Coord {
                x: 80.0 * factor,
                y: 20.0 * factor,
            },
            Coord {
                x: -80.0 * factor,
                y: -20.0 * factor,
            },
        ];

        if reverse {
            coords.reverse();
        }

        self.with("geometry", LineString::new(coords))
    }

    pub(super) fn build(self) -> LegendFeatureData {
        self.0
    }
}

#[derive(Default)]
pub(super) struct LegendItemDataBuilder(pub(super) LegendItemData);

impl LegendItemDataBuilder {
    fn with_layer(mut self, layer: impl Into<String>, features: Vec<LegendFeatureData>) -> Self {
        self.0.insert(layer.into(), features);
        self
    }

    pub(super) fn with_feature(self, layer: impl Into<String>, feature: LegendFeatureData) -> Self {
        self.with_layer(layer, vec![feature])
    }

    pub(super) fn build(self) -> LegendItemData {
        self.0
    }
}

pub(super) fn legend_feature_data_builder() -> LegendFeatureDataBuilder {
    LegendFeatureDataBuilder::default()
}

pub(super) fn legend_item_data_builder() -> LegendItemDataBuilder {
    LegendItemDataBuilder::default()
}

pub(super) fn with_landcover(
    typ: &'static str,
    zoom: u8,
    mode: LegendMode,
) -> LegendItemDataBuilder {
    let b = legend_item_data_builder();

    if mode == LegendMode::Normal {
        b.with_feature(
            "landcovers",
            legend_feature_data_builder()
                .with("type", typ)
                .with("name", "")
                .with("geometry", polygon(true, zoom))
                .build(),
        )
    } else {
        b
    }
}

pub(super) fn polygon(skew: bool, zoom: u8) -> Polygon {
    let factor = (19.0 - zoom as f64).exp2();

    let ssx = if skew { 2.0 } else { 0.0 };
    let ssy = if skew { 1.0 } else { 0.0 };

    let xx = 12.0;
    let yy = 5.0;

    Polygon::new(
        LineString::new(vec![
            Coord {
                x: factor * -xx,
                y: factor * (-yy - ssy),
            },
            Coord {
                x: factor * (-xx - ssx),
                y: factor * yy,
            },
            Coord {
                x: factor * xx,
                y: factor * (yy + ssy),
            },
            Coord {
                x: factor * (xx + ssx),
                y: factor * -yy,
            },
            Coord {
                x: factor * -xx,
                y: factor * (-yy - ssy),
            },
        ]),
        vec![],
    )
}

pub(super) fn build_tags_map(
    tags: Vec<(&'static str, &'static str)>,
) -> IndexMap<&'static str, &'static str> {
    let mut map = IndexMap::with_capacity(tags.len());

    for (k, v) in tags {
        map.insert(k, v);
    }

    map
}

pub(super) fn leak_str(value: &str) -> &'static str {
    value.to_string().leak()
}
