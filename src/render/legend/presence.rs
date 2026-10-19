use super::{LegendItem, clamp_zoom, legend_items, legend_sample_size};
use crate::render::{
    Feature, Layers,
    draw::{
        font_options::{FontAndLayoutOptions, ShapeKey},
        text::measure_line,
    },
    layers::{
        Look, poi_look,
        roads::{RoadStyle, style},
    },
};
use cosmic_text::Style;
use geo::{Intersects, Rect};
use std::{
    borrow::Cow,
    cell::OnceCell,
    collections::{HashMap, HashSet},
    ops::Deref,
};

/// A further condition a fetched row has to meet to stand for a legend item.
pub type RowTest = Box<dyn Fn(&Row) -> bool + Send + Sync>;

/// A fetched row, with what several rules read of it worked out once.
pub struct Row<'a> {
    feature: &'a Feature,
    zoom: u8,
    extra: OnceCell<Option<HashMap<String, Option<String>>>>,
    look: OnceCell<Option<Look<'a>>>,
    road_style: OnceCell<Option<RoadStyle>>,
}

impl<'a> Row<'a> {
    const fn new(feature: &'a Feature, zoom: u8) -> Self {
        Self {
            feature,
            zoom,
            extra: OnceCell::new(),
            look: OnceCell::new(),
            road_style: OnceCell::new(),
        }
    }

    pub const fn zoom(&self) -> u8 {
        self.zoom
    }

    pub fn extra(&self) -> Option<&HashMap<String, Option<String>>> {
        self.extra
            .get_or_init(|| self.feature.get_hstore("extra").ok())
            .as_ref()
    }

    /// How the POI layer draws the row.
    pub fn poi_look(&self) -> Option<&Look<'a>> {
        self.look
            .get_or_init(|| {
                poi_look(
                    self.feature.get_string("type").ok()?,
                    self.extra()?,
                    self.zoom,
                )
            })
            .as_ref()
    }
}

impl Row<'_> {
    /// How the roads layer draws the row.
    pub fn road_style(&self) -> Option<RoadStyle> {
        *self
            .road_style
            .get_or_init(|| style(self.zoom, self.feature).ok().flatten())
    }

    /// Whether the roads layer draws the row in one of `styles`.
    pub fn drawn_as(&self, styles: &[RoadStyle]) -> bool {
        self.road_style()
            .is_some_and(|style| styles.contains(&style))
    }
}

impl Deref for Row<'_> {
    type Target = Feature;

    fn deref(&self) -> &Feature {
        self.feature
    }
}

/// One way a legend item shows up among the rows a map render fetches.
pub struct Presence {
    /// The pipeline stage whose rows are looked at, by the name it registers under.
    pub stage: &'static str,
    /// Values of the row's `type` column that can stand for the item; empty takes
    /// a row of any type.
    pub types: Vec<&'static str>,
    pub when: Option<RowTest>,
    /// Whether a matching row shows the item is drawn. A label stage's rows only
    /// lend a name.
    pub marks: bool,
    pub lends: Option<Lend>,
}

/// A row's column whose text a sample takes in place of one of its props.
#[derive(Clone, Copy)]
pub struct Lend {
    pub from: &'static str,
    pub into: &'static str,
    /// For a feature's name, the font the stage draws a row's name in, or `None`
    /// where it draws none: a name has to fit on one line, and brings a POI's
    /// elevation along. Refs carry symbols.
    pub name_font: Option<fn(&Row) -> Option<FontAndLayoutOptions>>,
    /// The part of the text the sample can show.
    pub pick: Option<fn(&str) -> String>,
}

impl Lend {
    /// A name in the labels' usual font.
    pub const NAME: Self = Self::name(|_| Some(FontAndLayoutOptions::default()));

    /// A name in italics, as water and areas are named.
    pub const ITALIC_NAME: Self = Self::name(|_| {
        Some(FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        })
    });

    pub const fn name(font: fn(&Row) -> Option<FontAndLayoutOptions>) -> Self {
        Self {
            from: "name",
            into: "name",
            name_font: Some(font),
            pick: None,
        }
    }
}

type Candidate = (usize, &'static Presence);

/// Margin a name keeps from the sample's sides.
const NAME_PADDING: f64 = 8.0;

#[derive(Default)]
struct Stage {
    by_type: HashMap<&'static str, Vec<Candidate>>,
    any: Vec<Candidate>,
}

/// A legend item the map drew.
pub struct Found {
    pub item: &'static LegendItem<'static>,
    pub lent: Option<Lent>,
}

/// What a drawn feature lends its item's sample.
#[derive(Clone)]
pub struct Lent {
    /// The prop the text replaces.
    pub column: &'static str,
    pub text: String,
    /// A POI's elevation, drawn under its name.
    pub ele: Option<String>,
}

/// The legend items a map render drew, told from the rows its stages fetched.
///
/// A row is counted once fetched and inside the image, so a POI that lost its
/// place to a collision still lists its item.
pub struct LegendPresence {
    items: &'static [LegendItem<'static>],
    zoom: u8,
    /// The image; queries fetch well around it, to keep labels at its edges whole.
    bbox: Rect<f64>,
    stages: HashMap<&'static str, Stage>,
    found: HashSet<usize>,
    lending: Lending,
}

/// The texts drawn features lend the items' samples.
struct Lending {
    /// Room a sample has for a name on one line.
    name_width: f64,
    lent: HashMap<usize, Lent>,
    /// Whether a name fits in a font, for the names already measured; the font is
    /// keyed with no text of its own.
    fits: HashMap<String, Vec<(ShapeKey, bool)>>,
}

impl Lending {
    /// Keeps the shortest text an item is offered, the likeliest to fit its sample.
    /// Whether `text` would replace what the item holds: it says something, and in
    /// fewer characters.
    fn wants(&self, idx: usize, text: &str) -> bool {
        // Not trimmed: a route's refs open with a tab where it has no name.
        !text.trim().is_empty()
            && self
                .lent
                .get(&idx)
                .is_none_or(|held| text.chars().count() < held.text.chars().count())
    }

    /// Takes `text`, which [`Self::wants`], unless it is a name too wide to fit.
    fn offer(&mut self, idx: usize, lend: Lend, text: &str, row: &Row) {
        // A second line would be cut off by the sample's edge.
        if let Some(font) = lend.name_font {
            let Some(flo) = font(row) else {
                return;
            };

            let key = ShapeKey::new("", &flo);

            let known = self
                .fits
                .get(text)
                .and_then(|fonts| fonts.iter().find(|(font, _)| *font == key))
                .map(|(_, fits)| *fits);

            let fits = known.unwrap_or_else(|| {
                let fits = measure_line(text, &flo) <= self.name_width;

                self.fits
                    .entry(text.to_owned())
                    .or_default()
                    .push((key, fits));

                fits
            });

            if !fits {
                return;
            }
        }

        let ele = lend
            .name_font
            .and_then(|_| row.extra()?.get("ele").cloned().flatten())
            .filter(|ele| !ele.trim().is_empty());

        self.lent.insert(
            idx,
            Lent {
                column: lend.into,
                text: text.to_owned(),
                ele,
            },
        );
    }
}

impl LegendPresence {
    /// Looks only for the items the map draws at `zoom` with `layers`.
    pub fn new(zoom: u8, layers: &Layers, bbox: Rect<f64>) -> Self {
        let zoom = clamp_zoom(zoom);

        let items = legend_items(Some(zoom), false);

        let mut stages: HashMap<&'static str, Stage> = HashMap::new();

        for (idx, item) in items.iter().enumerate() {
            if !item.zooms.contains(&zoom) || !item.drawn_by(layers) {
                continue;
            }

            for presence in &item.presence {
                let stage = stages.entry(presence.stage).or_default();

                if presence.types.is_empty() {
                    stage.any.push((idx, presence));
                } else {
                    for typ in &presence.types {
                        stage.by_type.entry(typ).or_default().push((idx, presence));
                    }
                }
            }
        }

        Self {
            items,
            zoom,
            bbox,
            stages,
            found: HashSet::new(),
            lending: Lending {
                name_width: f64::from(legend_sample_size(zoom).width) - NAME_PADDING,
                lent: HashMap::new(),
                fits: HashMap::new(),
            },
        }
    }

    pub fn observe(&mut self, stage: &str, rows: &[Feature]) {
        let Some(stage) = self.stages.get(stage) else {
            return;
        };

        for feature in rows {
            let row = Row::new(feature, self.zoom);

            let mut inside = None;

            let typed = if stage.by_type.is_empty() {
                None
            } else {
                row.get_string("type")
                    .ok()
                    .and_then(|typ| stage.by_type.get(typ))
            };

            for (idx, presence) in typed.into_iter().flatten().chain(&stage.any) {
                let marks = presence.marks && !self.found.contains(idx);

                // Checked before the geometry, which most rows need not decode.
                let lent = presence.lends.and_then(|lend| {
                    let text = row.get_string(lend.from).ok()?;

                    let text = lend
                        .pick
                        .map_or(Cow::Borrowed(text), |pick| Cow::Owned(pick(text)));

                    self.lending.wants(*idx, &text).then_some((lend, text))
                });

                if !marks && lent.is_none() {
                    continue;
                }

                if presence.when.as_ref().is_some_and(|when| !when(&row)) {
                    continue;
                }

                let inside = *inside.get_or_insert_with(|| {
                    row.get_geometry()
                        .is_ok_and(|geometry| geometry.intersects(&self.bbox))
                });

                if !inside {
                    continue;
                }

                if marks {
                    self.found.insert(*idx);
                }

                if let Some((lend, text)) = lent {
                    self.lending.offer(*idx, lend, &text, &row);
                }
            }
        }
    }

    /// The items seen, in catalogue order.
    pub fn found(&self) -> Vec<Found> {
        self.items
            .iter()
            .enumerate()
            .filter(|(idx, _)| self.found.contains(idx))
            .map(|(idx, item)| Found {
                item,
                lent: self.lending.lent.get(&idx).cloned(),
            })
            .collect()
    }
}
