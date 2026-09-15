use crate::render::{
    collision::Collision,
    colors::{self, Color},
    draw::{
        font_options::{FontAndLayoutOptions, label_attrs, label_text},
        font_system::{
            HaloPaint, draw_with_halo, glyph_outline, stamp_outline, with_font_system,
            with_scale_context,
        },
    },
};
use cairo::Context;
use cosmic_text::{AttrsList, Buffer, BufferLine, LineEnding, Metrics, Shaping, Wrap};
use geo::{Point, Rect};

#[derive(Copy, Clone)]
pub struct TextOptions<'a> {
    pub alpha: f64,
    pub color: Color,
    pub halo_color: Color,
    pub halo_opacity: f64,
    pub halo_width: f64,
    pub placements: &'a [(f64, f64)],
    pub flo: FontAndLayoutOptions,
    pub valign_by_placement: bool,
    pub omit_bbox: Option<usize>,
    /// Scale factor for the font size of all lines after the first.
    /// `None` (default) = all lines use `flo.size`. Used e.g. for POI
    /// labels where the elevation line is drawn smaller than the name.
    pub sub_size_scale: Option<f32>,
}

impl Default for TextOptions<'_> {
    fn default() -> Self {
        TextOptions {
            alpha: 1.0,
            color: colors::BLACK,
            halo_color: colors::WHITE,
            halo_opacity: 0.75,
            halo_width: 1.5,
            flo: FontAndLayoutOptions::default(),
            placements: &[
                (0.0, 0.0),
                (0.0, 3.0),
                (0.0, -3.0),
                (0.0, 6.0),
                (0.0, -6.0),
                (0.0, 9.0),
                (0.0, -9.0),
            ],
            valign_by_placement: false,
            omit_bbox: None,
            sub_size_scale: None,
        }
    }
}

impl From<&TextOptions<'_>> for HaloPaint {
    fn from(options: &TextOptions<'_>) -> Self {
        Self {
            color: options.color,
            halo_color: options.halo_color,
            halo_opacity: options.halo_opacity,
            halo_width: options.halo_width,
            alpha: options.alpha,
        }
    }
}

pub fn draw_text(
    context: &Context,
    collision: Option<&mut Collision>,
    point: &Point,
    text: &str,
    options: &TextOptions,
) -> cairo::Result<Option<usize>> {
    if text.is_empty() {
        return Ok(Some(0));
    }

    let flo = &options.flo;

    let text = label_text(text, flo);

    let base_attrs = label_attrs(flo);

    let line_height = flo.size;
    let metrics = Metrics::new(flo.size as f32, line_height as f32);

    with_font_system(|font_system| {
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_wrap(Wrap::Word);

        #[allow(clippy::float_cmp)] // exact identity check: skip when sub-size scale is 1.0
        if let Some(scale) = options.sub_size_scale
            && scale > 0.0
            && scale != 1.0
        {
            let scaled_metrics =
                Metrics::new(flo.size as f32 * scale, line_height as f32 * scale);
            let sub_attrs = base_attrs.clone().metrics(scaled_metrics);

            let mut lines: Vec<BufferLine> = Vec::new();
            for (i, line_text) in text.split('\n').enumerate() {
                let attrs = if i == 0 { &base_attrs } else { &sub_attrs };
                let attrs_list = AttrsList::new(attrs);
                lines.push(BufferLine::new(
                    line_text.to_string(),
                    LineEnding::Lf,
                    attrs_list,
                    Shaping::Advanced,
                ));
            }
            buffer.lines = lines;
            buffer.set_size(Some(flo.max_width as f32), None);
            buffer.shape_until_scroll(font_system, true);
        } else {
            let mut buf = buffer.borrow_with(font_system);
            buf.set_size(Some(flo.max_width as f32), None);
            buf.set_text(&text, &base_attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(true);
        }

        let lines = with_scale_context(|sc| compute_lines(&buffer, font_system, sc));

        if lines.is_empty() {
            return Ok(Some(0));
        }

        let Some(placed) = place(collision, point, &lines, options) else {
            return Ok(None);
        };

        context.status()?;

        draw_with_halo(context, &placed.boxes, &HaloPaint::from(options), || {
            with_scale_context(|scale_ctx| {
                for run in buffer.layout_runs() {
                    let line_x = (placed.layout_width - run.line_w as f64) / 2.0;

                    for glyph in run.glyphs {
                        let Some(outline) = glyph_outline(font_system, scale_ctx, glyph) else {
                            continue;
                        };

                        let gx = placed.x + line_x + glyph.x as f64;
                        let gy = placed.y + (run.line_y + glyph.y) as f64;

                        stamp_outline(context, &outline, gx, gy);
                    }
                }
            });
        })?;

        Ok(Some(placed.idx))
    })
}

struct LineInfo {
    line_y: f32,   // baseline y in layout coords
    line_w: f32,   // advance width (for centering)
    ink_left: f32, // ink extents in layout coords (Y-down)
    ink_right: f32,
    ink_top: f32,
    ink_bottom: f32,
    font_ascent: f32, // font's hhea ascent at this line's font size (constant per font)
    font_descent: f32, // font's hhea descent (positive) at this line's font size
}

/// Compute per-line ink bounds by scaling each glyph outline and taking the
/// union of its bounding box. Outlines live in Y-up coords relative to the
/// glyph's pen position; we map to layout Y-down by flipping Y around the
/// baseline. Outlines come from the glyph cache, so drawing reuses them.
fn compute_lines(
    buffer: &Buffer,
    font_system: &mut cosmic_text::FontSystem,
    scale_ctx: &mut swash::scale::ScaleContext,
) -> Vec<LineInfo> {
    let mut lines: Vec<LineInfo> = Vec::new();
    for run in buffer.layout_runs() {
        let mut l = f32::INFINITY;
        let mut r = f32::NEG_INFINITY;
        let mut t = f32::INFINITY;
        let mut b = f32::NEG_INFINITY;

        // Font vmetrics from the first glyph of the run — used for the
        // placement anchor, which must not depend on per-string ink.
        let (font_ascent, font_descent) = run
            .glyphs
            .first()
            .and_then(|g| {
                font_system.get_font(g.font_id, g.font_weight).map(|f| {
                    let m = f.as_swash().metrics(&[]);
                    let scale = g.font_size / m.units_per_em as f32;
                    (m.ascent * scale, m.descent.abs() * scale)
                })
            })
            .unwrap_or((0.0, 0.0));

        for glyph in run.glyphs {
            let Some(outline) = glyph_outline(font_system, scale_ctx, glyph) else {
                continue;
            };

            let bb = outline.bounds();
            let gx = glyph.x;
            let gy = run.line_y + glyph.y;

            l = l.min(gx + bb.min.x);
            r = r.max(gx + bb.max.x);
            t = t.min(gy - bb.max.y);
            b = b.max(gy - bb.min.y);
        }

        // Empty or whitespace-only run: contribute advance but no ink bounds.
        if !l.is_finite() {
            if let Some(last) = lines.last_mut()
                && (last.line_y - run.line_y).abs() < f32::EPSILON
            {
                last.line_w = last.line_w.max(run.line_w);
            }
            continue;
        }

        if let Some(last) = lines.last_mut()
            && (last.line_y - run.line_y).abs() < f32::EPSILON
        {
            last.line_w = last.line_w.max(run.line_w);
            last.ink_left = last.ink_left.min(l);
            last.ink_right = last.ink_right.max(r);
            last.ink_top = last.ink_top.min(t);
            last.ink_bottom = last.ink_bottom.max(b);
            last.font_ascent = last.font_ascent.max(font_ascent);
            last.font_descent = last.font_descent.max(font_descent);
        } else {
            lines.push(LineInfo {
                line_y: run.line_y,
                line_w: run.line_w,
                ink_left: l,
                ink_right: r,
                ink_top: t,
                ink_bottom: b,
                font_ascent,
                font_descent,
            });
        }
    }
    lines
}

/// Where a label went, with the ink boxes (halo included) it added to the collision set.
struct Placed {
    x: f64,
    y: f64,
    idx: usize,
    layout_width: f64,
    boxes: Vec<Rect>,
}

/// Tries the placements in order and takes the first that doesn't collide.
fn place(
    collision: Option<&mut Collision>,
    point: &Point,
    lines: &[LineInfo],
    options: &TextOptions,
) -> Option<Placed> {
    let halo_width = options.halo_width;

    let layout_width = lines.iter().map(|l| l.line_w).fold(0.0f32, f32::max) as f64;

    let layout_min_top = lines
        .iter()
        .map(|l| l.ink_top)
        .fold(f32::INFINITY, f32::min) as f64;

    let layout_max_bottom = lines
        .iter()
        .map(|l| l.ink_bottom)
        .fold(f32::NEG_INFINITY, f32::max) as f64;

    let center = layout_min_top + (layout_max_bottom - layout_min_top) / 2.0;

    let first = lines.first().expect("lines is non-empty");
    let last = lines.last().expect("lines is non-empty");
    let first_baseline = first.line_y as f64;
    let last_baseline = last.line_y as f64;
    // Anchor for "label above" uses the baseline directly (pango semantics:
    // baseline sits at point+dy). Anchor for "label below" uses the font's
    // metric ascent so different strings line up consistently from the icon.
    // Collision rect still uses ink below.
    let cap_height = first.font_ascent as f64;

    let x_base = point.x() - layout_width / 2.0;

    let (idx, x, y, boxes) =
        options
            .placements
            .iter()
            .enumerate()
            .find_map(|(i, &(dx, dy))| {
                let y_anchor = match (options.valign_by_placement, dy) {
                    (true, dy) if dy > 0.0 => first_baseline - cap_height,
                    (true, dy) if dy < 0.0 => last_baseline,
                    _ => center,
                };

                let y = dy + point.y() - y_anchor;
                let x = dx + x_base;

                let boxes = lines
                    .iter()
                    .map(|line| {
                        let line_x = (layout_width - line.line_w as f64) / 2.0;

                        let ci = Rect::new(
                            (
                                x + line_x + line.ink_left as f64 - halo_width,
                                y + line.ink_top as f64 - halo_width,
                            ),
                            (
                                x + line_x + line.ink_right as f64 + halo_width,
                                y + line.ink_bottom as f64 + halo_width,
                            ),
                        );

                        let collides = collision
                            .as_deref()
                            .is_some_and(|collision| collision.collides(&ci, options.omit_bbox));

                        (!collides).then_some(ci)
                    })
                    .collect::<Option<Vec<_>>>()?;

                Some((i + 1, x, y, boxes))
            })?;

    if let Some(collision) = collision {
        for bb in &boxes {
            let _ = collision.add(*bb);
        }
    }

    Some(Placed {
        x,
        y,
        idx,
        layout_width,
        boxes,
    })
}
