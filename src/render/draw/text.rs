use crate::render::{
    collision::Collision,
    colors::{self, Color, ContextExt},
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        font_system::{with_font_system, with_scale_context},
    },
};
use cairo::Context;
use cosmic_text::{
    Attrs, AttrsList, Buffer, BufferLine, Family, LineEnding, Metrics, Shaping, Weight as CtWeight,
    Wrap,
};
use geo::{Point, Rect};
use std::borrow::Cow;
use swash::zeno::Verb;

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
            flo: Default::default(),
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

    let TextOptions {
        alpha,
        color,
        halo_color,
        halo_opacity,
        halo_width,
        placements,
        flo,
        valign_by_placement,
        omit_bbox,
        sub_size_scale,
    } = options;

    let FontAndLayoutOptions {
        letter_spacing,
        max_width,
        narrow,
        size,
        uppercase,
        ..
    } = *flo;

    let text: Cow<str> = if uppercase {
        Cow::Owned(text.to_uppercase())
    } else {
        Cow::Borrowed(text)
    };

    let family = Family::Name(if narrow {
        "PT Sans Narrow"
    } else {
        "PT Sans"
    });

    let base_attrs = Attrs::new()
        .family(family)
        .weight(CtWeight(flo.ct_weight_u16()))
        .style(flo.ct_style())
        .letter_spacing((letter_spacing / size.max(0.0001)) as f32);

    let line_height = size;
    let metrics = Metrics::new(size as f32, line_height as f32);

    let m = with_font_system(|font_system| {
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_wrap(Wrap::Word);

        if let Some(scale) = sub_size_scale
            && *scale > 0.0
            && *scale != 1.0
        {
            let scaled_metrics = Metrics::new(size as f32 * scale, line_height as f32 * scale);
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
            buffer.set_size(Some(max_width as f32), None);
            buffer.shape_until_scroll(font_system, true);
        } else {
            let mut buf = buffer.borrow_with(font_system);
            buf.set_size(Some(max_width as f32), None);
            buf.set_text(&text, &base_attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(true);
        }

        place_and_draw(
            context,
            collision,
            point,
            &buffer,
            font_system,
            halo_width,
            placements,
            valign_by_placement,
            omit_bbox,
        )
    })?;

    let placement_idx = match m {
        Some(idx) => idx,
        None => return Ok(None),
    };

    context.status()?;

    context.push_group();

    context.set_source_color_a(*halo_color, *halo_opacity);
    context.set_dash(&[], 0.0);
    context.set_line_join(cairo::LineJoin::Round);
    context.set_line_width(halo_width * 2.0);
    context.stroke_preserve()?;
    context.set_source_color(*color);

    context.fill()?;

    context.pop_group_to_source()?;

    context.paint_with_alpha(*alpha)?;

    context.set_source_rgb(1.0, 0.0, 0.0);
    context.arc(point.x(), point.y(), 1.5, 0.0, std::f64::consts::TAU);
    context.fill()?;

    Ok(Some(placement_idx))
}

struct LineInfo {
    line_y: f32,       // baseline y in layout coords
    line_w: f32,       // advance width (for centering)
    ink_left: f32,     // ink extents in layout coords (Y-down)
    ink_right: f32,
    ink_top: f32,
    ink_bottom: f32,
    font_ascent: f32,  // font's hhea ascent at this line's font size (constant per font)
    font_descent: f32, // font's hhea descent (positive) at this line's font size
}

/// Compute per-line ink bounds by scaling each glyph outline and taking the
/// union of its bounding box. Outlines live in Y-up coords relative to the
/// glyph's pen position; we map to layout Y-down by flipping Y around the
/// baseline. Uses the thread-local `ScaleContext` so the scaled outlines
/// stay cached for the subsequent render pass.
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

        for glyph in run.glyphs.iter() {
            let Some(font) = font_system.get_font(glyph.font_id, glyph.font_weight) else {
                continue;
            };
            let mut scaler = scale_ctx
                .builder(font.as_swash())
                .size(glyph.font_size)
                .build();
            let Some(outline) = scaler.scale_outline(glyph.glyph_id) else {
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

#[allow(clippy::too_many_arguments)]
fn place_and_draw(
    context: &Context,
    collision: Option<&mut Collision>,
    point: &Point,
    buffer: &Buffer,
    font_system: &mut cosmic_text::FontSystem,
    halo_width: &f64,
    placements: &[(f64, f64)],
    valign_by_placement: &bool,
    omit_bbox: &Option<usize>,
) -> cairo::Result<Option<usize>> {
    let lines = with_scale_context(|sc| compute_lines(buffer, font_system, sc));

    if lines.is_empty() {
        return Ok(Some(0));
    }

    let layout_width = lines.iter().map(|l| l.line_w).fold(0.0f32, f32::max) as f64;
    let layout_min_top = lines.iter().map(|l| l.ink_top).fold(f32::INFINITY, f32::min) as f64;
    let layout_max_bottom = lines
        .iter()
        .map(|l| l.ink_bottom)
        .fold(f32::NEG_INFINITY, f32::max) as f64;
    let layout_y = layout_min_top;
    let layout_height = layout_max_bottom - layout_min_top;

    let first = lines.first().unwrap();
    let last = lines.last().unwrap();
    let first_baseline = first.line_y as f64;
    let last_baseline = last.line_y as f64;
    // Anchor for "label above" uses the baseline directly (pango semantics:
    // baseline sits at point+dy). Anchor for "label below" uses the font's
    // metric ascent so different strings line up consistently from the icon.
    // Collision rect still uses ink below.
    let cap_height = first.font_ascent as f64;

    let x_base = point.x() - layout_width / 2.0;

    let mut m: Option<(f64, f64, usize)> = None;
    let mut i: usize = 0;

    let mut collision = collision;

    'outer: for &(dx, dy) in placements {
        i += 1;

        let y_anchor = if *valign_by_placement {
            if dy > 0.0 {
                first_baseline - cap_height
            } else if dy < 0.0 {
                last_baseline
            } else {
                layout_y + layout_height / 2.0
            }
        } else {
            layout_y + layout_height / 2.0
        };

        let y = dy + point.y() - y_anchor;
        let x = dx + x_base;

        let mut items = Vec::new();

        let mut collided = false;
        for line in &lines {
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

            if let Some(ref collision) = collision {
                if let Some(omit_idx) = *omit_bbox {
                    if collision.collides_with_exclusion(&ci, omit_idx) {
                        collided = true;
                        break;
                    }
                } else if collision.collides(&ci) {
                    collided = true;
                    break;
                }
            }

            items.push(ci);
        }

        if collided {
            continue 'outer;
        }

        if let Some(ref mut collision) = collision {
            for item in items {
                let _ = collision.add(item);
            }
        }

        m = Some((x, y, i));
        break;
    }

    let Some((tx, ty, _)) = m else {
        return Ok(None);
    };

    context.new_path();

    with_scale_context(|scale_ctx| {
        for run in buffer.layout_runs() {
            let line_x = (layout_width - run.line_w as f64) / 2.0;

            for glyph in run.glyphs.iter() {
                let font = font_system
                    .get_font(glyph.font_id, glyph.font_weight)
                    .unwrap();
                let font_ref = font.as_swash();

                let mut scaler = scale_ctx.builder(font_ref).size(glyph.font_size).build();

                let Some(outline) = scaler.scale_outline(glyph.glyph_id) else {
                    continue;
                };

                let gx = tx + line_x + glyph.x as f64;
                let gy = ty + (run.line_y + glyph.y) as f64;

                let points = outline.points();
                let mut idx = 0;
                let mut cur = (0.0_f64, 0.0_f64);

                for verb in outline.verbs() {
                    match verb {
                        Verb::MoveTo => {
                            let p = points[idx];
                            idx += 1;
                            let (x, y) = (gx + p.x as f64, gy - p.y as f64);
                            context.move_to(x, y);
                            cur = (x, y);
                        }
                        Verb::LineTo => {
                            let p = points[idx];
                            idx += 1;
                            let (x, y) = (gx + p.x as f64, gy - p.y as f64);
                            context.line_to(x, y);
                            cur = (x, y);
                        }
                        Verb::CurveTo => {
                            let p1 = points[idx];
                            let p2 = points[idx + 1];
                            let p3 = points[idx + 2];
                            idx += 3;
                            let (x1, y1) = (gx + p1.x as f64, gy - p1.y as f64);
                            let (x2, y2) = (gx + p2.x as f64, gy - p2.y as f64);
                            let (x3, y3) = (gx + p3.x as f64, gy - p3.y as f64);
                            context.curve_to(x1, y1, x2, y2, x3, y3);
                            cur = (x3, y3);
                        }
                        Verb::QuadTo => {
                            let p1 = points[idx];
                            let p2 = points[idx + 1];
                            idx += 2;
                            let (x1, y1) = (gx + p1.x as f64, gy - p1.y as f64);
                            let (x2, y2) = (gx + p2.x as f64, gy - p2.y as f64);
                            let (x0, y0) = cur;
                            let c1x = x0 + 2.0 / 3.0 * (x1 - x0);
                            let c1y = y0 + 2.0 / 3.0 * (y1 - y0);
                            let c2x = x2 + 2.0 / 3.0 * (x1 - x2);
                            let c2y = y2 + 2.0 / 3.0 * (y1 - y2);
                            context.curve_to(c1x, c1y, c2x, c2y, x2, y2);
                            cur = (x2, y2);
                        }
                        Verb::Close => {
                            context.close_path();
                        }
                    }
                }
            }
        }
    });

    Ok(m.map(|(_, _, idx)| idx))
}
