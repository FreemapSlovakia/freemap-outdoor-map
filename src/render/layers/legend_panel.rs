use crate::render::{
    colors::{self, ContextExt},
    draw::{
        font_options::FontAndLayoutOptions,
        font_options::label_attrs,
        font_system::with_font_system,
        text::{TextOptions, draw_text, measure_line},
    },
    layers::Category,
    render_request::LegendDecoration,
    size::Size,
};
use cairo::{Context, RecordingSurface};
use cosmic_text::{Buffer, Metrics, Shaping, Weight, Wrap};
use geo::Point;

const PADDING: f64 = 12.0;

/// Samples are drawn at this fraction of the size the legend endpoint renders them
/// at, which leaves room for a label beside each.
const SAMPLE_SCALE: f64 = 0.75;

const SAMPLE_GAP: f64 = 8.0;

const COLUMN_GAP: f64 = 16.0;

const ROW_GAP: f64 = 4.0;

const LABEL_SIZE: f64 = 12.0;

const LINE_HEIGHT: f64 = LABEL_SIZE * 1.2;

/// A longer label wraps.
const MAX_LABEL_WIDTH: f64 = 180.0;

const HEADING_SIZE: f64 = 13.0;

/// Space above a heading that does not start a column.
const HEADING_GAP: f64 = 8.0;

const TITLE_SIZE: f64 = 16.0;

/// A legend item the map drew, ready to be listed.
#[derive(Clone)]
pub struct Entry {
    pub category: Category,
    pub label: String,
    /// The caller's handle on the item, which [`Panel::add_samples`] asks by.
    pub id: usize,
}

/// A line of text and its width.
struct Line {
    text: String,
    width: f64,
}

enum Row {
    Heading(Vec<Line>),
    Item {
        lines: Vec<Line>,
        id: usize,
        /// In logical pixels times the render's scale.
        sample: Option<RecordingSurface>,
    },
}

impl Row {
    fn height(&self, sample_height: f64) -> f64 {
        match self {
            Self::Heading(lines) => HEADING_SIZE * (lines.len() as f64).mul_add(1.2, 0.2),
            Self::Item { lines, .. } => sample_height.max(lines.len() as f64 * LINE_HEIGHT),
        }
    }
}

/// The legend laid out in columns as tall as the map beside it.
pub struct Panel {
    pub width: f64,
    title: Option<Line>,
    sample: (f64, f64),
    column_width: f64,
    /// Rows by column, each with its top edge.
    columns: Vec<Vec<(f64, Row)>>,
}

impl Panel {
    /// Lays out `entries`, with samples of `sample_size`, beside a map of `map_size`.
    /// The panel is never wider than the map, and nothing runs past its bottom: what
    /// does not fit is left out. `None` when nothing fits.
    pub fn layout(
        decoration: &LegendDecoration,
        mut entries: Vec<Entry>,
        sample_size: Size<u32>,
        map_size: Size<u32>,
    ) -> Option<Self> {
        let sample = (
            f64::from(sample_size.width) * SAMPLE_SCALE,
            f64::from(sample_size.height) * SAMPLE_SCALE,
        );

        let title = decoration.title.as_ref().map(|title| Line {
            text: title.clone(),
            width: measure_line(title, &title_flo()),
        });

        let top = PADDING + title.as_ref().map_or(0.0, |_| TITLE_SIZE * 1.6);

        let bottom = f64::from(map_size.height) - PADDING;

        // Sections in the order the web client lists them, items in catalogue order.
        entries.sort_by_key(|entry| entry.category);

        let mut rows = Vec::new();

        let mut category = None;

        let mut headed = false;

        for entry in entries {
            if category != Some(entry.category) {
                category = Some(entry.category);

                let heading = decoration.categories.get(&entry.category.key());

                headed = heading.is_some();

                if let Some(heading) = heading {
                    rows.push(Row::Heading(wrap(heading, &heading_flo())));
                }
            }

            // Left out, like an item without a label.
            if headed {
                rows.push(Row::Item {
                    lines: wrap(&entry.label, &label_flo()),
                    id: entry.id,
                    sample: None,
                });
            }
        }

        // A heading spans the column; a label sits beside its sample.
        let column_width = |rows: &mut dyn Iterator<Item = &Row>| {
            rows.flat_map(|row| match row {
                Row::Heading(lines) => lines.iter().map(|line| line.width).collect::<Vec<_>>(),
                Row::Item { lines, .. } => lines
                    .iter()
                    .map(|line| sample.0 + SAMPLE_GAP + line.width)
                    .collect(),
            })
            .fold(0.0, f64::max)
            .min(sample.0 + SAMPLE_GAP + MAX_LABEL_WIDTH)
        };

        let available = PADDING.mul_add(-2.0, f64::from(map_size.width));

        // As wide as any row would need; the rows placed may need less.
        let widest = column_width(&mut rows.iter());

        let max_columns = ((available + COLUMN_GAP) / (widest + COLUMN_GAP)).floor();

        if max_columns < 1.0 {
            return None;
        }

        let max_columns = max_columns as usize;

        let mut columns: Vec<Vec<(f64, Row)>> = vec![Vec::new()];

        let mut y = top;

        let mut rows = rows.into_iter().peekable();

        // In a section whose heading found no place.
        let mut skipping = false;

        while let Some(row) = rows.next() {
            let heading = matches!(row, Row::Heading(_));

            if heading {
                skipping = false;
            } else if skipping {
                continue;
            }

            let height = row.height(sample.1);

            // A heading goes along with its first item.
            let block = height
                + match rows.peek() {
                    Some(next) if heading => ROW_GAP + next.height(sample.1),
                    _ => 0.0,
                };

            let gap = if heading && !columns.last()?.is_empty() {
                HEADING_GAP
            } else {
                0.0
            };

            if y + gap + block > bottom {
                // What no column can hold is left out, a heading with its section.
                if top + block > bottom {
                    skipping = heading;

                    continue;
                }

                if columns.len() == max_columns {
                    break;
                }

                columns.push(Vec::new());

                y = top;
            } else {
                y += gap;
            }

            columns.last_mut()?.push((y, row));

            y += height + ROW_GAP;
        }

        if columns[0].is_empty() {
            return None;
        }

        let column_width = column_width(&mut columns.iter().flatten().map(|(_, row)| row));

        let width = (columns.len() as f64)
            .mul_add(column_width + COLUMN_GAP, -COLUMN_GAP)
            .max(title.as_ref().map_or(0.0, |title| title.width))
            .min(available)
            + PADDING
            + PADDING;

        Some(Self {
            // Whole pixels, as the image is widened by it.
            width: width.ceil(),
            title,
            sample,
            column_width,
            columns,
        })
    }

    /// The ids of the entries that found a place.
    pub fn placed(&self) -> impl Iterator<Item = usize> + '_ {
        self.columns
            .iter()
            .flatten()
            .filter_map(|(_, row)| match row {
                Row::Item { id, .. } => Some(*id),
                Row::Heading(_) => None,
            })
    }

    /// Gives each entry that found a place its sample, by the entry's id.
    pub fn add_samples(&mut self, mut sample: impl FnMut(usize) -> Option<RecordingSurface>) {
        for (_, row) in self.columns.iter_mut().flatten() {
            if let Row::Item {
                id, sample: slot, ..
            } = row
            {
                *slot = sample(*id);
            }
        }
    }

    /// Draws the panel with its left edge at `left`, on a context in logical pixels.
    /// `scale` is the render's, which the samples were recorded at.
    pub fn draw(&self, context: &Context, left: f64, height: f64, scale: f64) -> cairo::Result<()> {
        context.save()?;

        context.rectangle(left, 0.0, self.width, height);
        context.set_source_color(colors::WHITE);
        context.fill_preserve()?;

        // A title wider than the map is cut at the panel's edge.
        context.clip();

        if let Some(title) = &self.title {
            draw_line(
                context,
                title,
                left + PADDING,
                PADDING + TITLE_SIZE / 2.0,
                &title_flo(),
            )?;
        }

        for (i, column) in self.columns.iter().enumerate() {
            let x = (i as f64).mul_add(self.column_width + COLUMN_GAP, left + PADDING);

            for (y, row) in column {
                match row {
                    Row::Heading(lines) => {
                        for (j, line) in lines.iter().enumerate() {
                            draw_line(
                                context,
                                line,
                                x,
                                (j as f64).mul_add(HEADING_SIZE * 1.2, y + HEADING_SIZE * 0.7),
                                &heading_flo(),
                            )?;
                        }
                    }
                    Row::Item { sample: None, .. } => {}
                    Row::Item {
                        lines,
                        sample: Some(sample),
                        ..
                    } => {
                        let row_height = row.height(self.sample.1);

                        let sample_y = y + (row_height - self.sample.1) / 2.0;

                        context.save()?;
                        context.rectangle(x, sample_y, self.sample.0, self.sample.1);
                        context.clip();
                        context.translate(x, sample_y);
                        context.scale(SAMPLE_SCALE / scale, SAMPLE_SCALE / scale);
                        context.set_source_surface(sample, 0.0, 0.0)?;
                        context.paint()?;
                        context.restore()?;

                        context.rectangle(
                            x + 0.25,
                            sample_y + 0.25,
                            self.sample.0 - 0.5,
                            self.sample.1 - 0.5,
                        );
                        context.set_source_color(colors::BLACK);
                        context.set_line_width(0.5);
                        context.stroke()?;

                        let text_top =
                            y + (lines.len() as f64).mul_add(-LINE_HEIGHT, row_height) / 2.0;

                        for (j, line) in lines.iter().enumerate() {
                            draw_line(
                                context,
                                line,
                                x + self.sample.0 + SAMPLE_GAP,
                                (j as f64).mul_add(LINE_HEIGHT, text_top + LINE_HEIGHT / 2.0),
                                &label_flo(),
                            )?;
                        }
                    }
                }
            }
        }

        context.restore()
    }
}

fn label_flo() -> FontAndLayoutOptions {
    FontAndLayoutOptions {
        size: LABEL_SIZE,
        max_width: f64::INFINITY,
        ..Default::default()
    }
}

fn title_flo() -> FontAndLayoutOptions {
    FontAndLayoutOptions {
        size: TITLE_SIZE,
        weight: Weight::BOLD,
        ..label_flo()
    }
}

fn heading_flo() -> FontAndLayoutOptions {
    FontAndLayoutOptions {
        size: HEADING_SIZE,
        weight: Weight::BOLD,
        ..label_flo()
    }
}

/// One line, its left edge at `x` and its middle at `y`.
fn draw_line(
    context: &Context,
    line: &Line,
    x: f64,
    y: f64,
    flo: &FontAndLayoutOptions,
) -> cairo::Result<()> {
    // `draw_text` centres on the point.
    draw_text(
        context,
        None,
        &Point::new(x + line.width / 2.0, y),
        &line.text,
        &TextOptions {
            placements: &[(0.0, 0.0)],
            flo: *flo,
            halo_width: 0.0,
            halo_opacity: 0.0,
            ..Default::default()
        },
    )?;

    Ok(())
}

/// `text` broken into lines no wider than [`MAX_LABEL_WIDTH`], at spaces where it
/// can be and between characters where a word alone is wider.
fn wrap(text: &str, flo: &FontAndLayoutOptions) -> Vec<Line> {
    with_font_system(|font_system| {
        let metrics = Metrics::new(flo.size as f32, flo.size as f32);
        let mut buffer = Buffer::new(font_system, metrics);

        let mut buf = buffer.borrow_with(font_system);
        buf.set_wrap(Wrap::WordOrGlyph);
        buf.set_size(Some(MAX_LABEL_WIDTH as f32), None);
        buf.set_text(text, &label_attrs(flo), Shaping::Advanced, None);
        buf.shape_until_scroll(true);

        buf.layout_runs()
            .filter_map(|run| {
                let start = run.glyphs.first()?.start;
                let end = run.glyphs.last()?.end;

                let text = run.text[start.min(end)..start.max(end)].trim();

                (!text.is_empty()).then(|| Line {
                    text: text.to_owned(),
                    width: f64::from(run.line_w),
                })
            })
            .collect()
    })
}
