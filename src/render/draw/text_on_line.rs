use crate::render::{
    collision::Collision,
    colors::{self, Color, ContextExt},
    draw::{
        create_pango_layout::{FontAndLayoutOptions, create_layout_checked},
        offset_line::offset_line_string,
    },
};
use cairo::Context;
use geo::Vector2DOps;
use geo::{Coord, Distance, Euclidean, InterpolatePoint, LineString, Rect};
use pangocairo::{
    functions::glyph_string_path,
    pango::{Font, GlyphItem, GlyphString, Layout, SCALE},
};
use std::f64::consts::{PI, TAU};

#[derive(Copy, Clone, Debug)]
pub struct TextOnLineOptions {
    pub upright: Upright,
    pub distribution: Distribution,
    pub alpha: f64,
    pub offset: f64,
    /// Keep the offset on the same side of the original baseline even when flipping for upright text.
    pub keep_offset_side: bool,
    pub color: Color,
    pub halo_color: Color,
    pub halo_opacity: f64,
    pub halo_width: f64,
    pub max_curvature_degrees: f64,
    pub concave_spacing_factor: f64,
    pub flo: FontAndLayoutOptions,
}

impl Default for TextOnLineOptions {
    fn default() -> Self {
        Self {
            upright: Upright::Auto,
            distribution: Distribution::Align {
                align: Align::Center,
                repeat: Repeat::None,
            },
            alpha: 1.0,
            offset: 0.0,
            keep_offset_side: false,
            color: colors::BLACK,
            halo_color: colors::WHITE,
            halo_opacity: 0.75,
            halo_width: 1.5,
            max_curvature_degrees: 45.0,
            concave_spacing_factor: 1.0,
            flo: FontAndLayoutOptions::default(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Upright {
    Left,
    #[allow(dead_code)]
    Right,
    Auto,
}

#[derive(Copy, Clone, Debug)]
pub enum Align {
    Left,
    Center,
    #[allow(dead_code)]
    Right,
}

#[derive(Copy, Clone, Debug)]
pub enum Repeat {
    None,
    Spaced(f64),
}

#[derive(Copy, Clone, Debug)]
pub enum Distribution {
    Align { align: Align, repeat: Repeat },
    Justify { min_spacing: Option<f64> },
}

fn normalize(v: Coord) -> Coord {
    v.try_normalize().unwrap_or(Coord { x: 0.0, y: 0.0 })
}

fn angle_between(a: Coord, b: Coord) -> f64 {
    a.wedge_product(b)
        .atan2(a.dot_product(b))
        .abs()
        .to_degrees()
}

fn normalize_angle(a: f64) -> f64 {
    if a > PI {
        a - TAU
    } else if a <= -PI {
        a + TAU
    } else {
        a
    }
}

fn adjust_upright_angle(angle: f64, upright: Upright) -> f64 {
    let a = normalize_angle(angle);

    match upright {
        Upright::Left => normalize_angle(a + PI),
        Upright::Right => a,
        Upright::Auto => {
            if a.abs() > PI / 2.0 {
                normalize_angle(a + PI)
            } else {
                a
            }
        }
    }
}

fn weighted_tangent_for_span(
    pts: &[Coord],
    cum: &[f64],
    span_start: f64,
    span_end: f64,
) -> Option<Coord> {
    if pts.len() < 2 {
        return None;
    }

    let mut accum = Coord { x: 0.0, y: 0.0 };
    let mut total = 0.0;

    for i in 0..pts.len() - 1 {
        let seg_start = cum[i];
        let seg_end = cum[i + 1];

        let overlap_start = span_start.max(seg_start);
        let overlap_end = span_end.min(seg_end);

        if overlap_end <= overlap_start {
            continue;
        }

        let weight = overlap_end - overlap_start;
        let tangent = normalize(pts[i + 1] - pts[i]);

        accum = accum + tangent * weight;
        total += weight;
    }

    if total == 0.0 {
        None
    } else {
        Some(normalize(accum))
    }
}

fn tangents_for_span(pts: &[Coord], cum: &[f64], span_start: f64, span_end: f64) -> Vec<Coord> {
    let mut result = Vec::new();

    for i in 0..pts.len() - 1 {
        let seg_start = cum[i];
        let seg_end = cum[i + 1];

        let overlap_start = span_start.max(seg_start);
        let overlap_end = span_end.min(seg_end);

        if overlap_end <= overlap_start {
            continue;
        }

        let tangent = normalize(pts[i + 1] - pts[i]);

        result.push(tangent);
    }

    result
}

fn cumulative_lengths(pts: &[Coord]) -> Vec<f64> {
    let mut result = Vec::with_capacity(pts.len());
    let mut total = 0.0;
    result.push(0.0);
    for window in pts.windows(2) {
        total += Euclidean.distance(window[0], window[1]);
        result.push(total);
    }
    result
}

fn position_at(pts: &[Coord], cum: &[f64], dist: f64) -> Option<(Coord, Coord)> {
    if pts.len() < 2 {
        return None;
    }

    if dist <= 0.0 {
        let tangent = normalize(pts[1] - pts[0]);
        return Some((pts[0], tangent));
    }

    if let Some(total) = cum.last()
        && dist >= *total
    {
        let len = pts.len();
        let tangent = normalize(pts[len - 1] - pts[len - 2]);
        return Some((pts[len - 1], tangent));
    }

    let mut idx = 0;
    while idx + 1 < cum.len() && cum[idx + 1] < dist {
        idx += 1;
    }

    let seg_len = cum[idx + 1] - cum[idx];
    if seg_len == 0.0 {
        return None;
    }

    let t = (dist - cum[idx]) / seg_len;
    let p1 = pts[idx];
    let p2 = pts[idx + 1];
    let pos = Euclidean.point_at_ratio_between(p1.into(), p2.into(), t).0;
    let tangent = normalize(p2 - p1);

    Some((pos, tangent))
}

fn trim_line_to_span(pts: &[Coord], cum: &[f64], span_start: f64, span_end: f64) -> Vec<Coord> {
    if pts.len() < 2 || span_end <= span_start {
        return Vec::new();
    }

    let total = *cum.last().unwrap_or(&0.0);
    if total == 0.0 {
        return Vec::new();
    }

    let start = span_start.clamp(0.0, total);
    let end = span_end.clamp(0.0, total);
    if end <= start {
        return Vec::new();
    }

    let mut trimmed = Vec::new();

    if let Some((p, _)) = position_at(pts, cum, start) {
        trimmed.push(p);
    }

    for i in 0..pts.len() - 1 {
        let seg_start = cum[i];
        let seg_end = cum[i + 1];
        if seg_end <= start || seg_start >= end {
            continue;
        }
        trimmed.push(pts[i + 1]);
    }

    if let Some((p, _)) = position_at(pts, cum, end)
        && trimmed.last().map(|q| *q != p).unwrap_or(true)
    {
        trimmed.push(p);
    }

    trimmed
}

fn bbox_intersects_clip(pts: &[Coord], clip: (f64, f64, f64, f64), padding: f64) -> bool {
    if pts.is_empty() {
        return false;
    }

    let (cx1, cy1, cx2, cy2) = clip;
    let min_cx = cx1.min(cx2);
    let max_cx = cx1.max(cx2);
    let min_cy = cy1.min(cy2);
    let max_cy = cy1.max(cy2);

    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;

    for p in pts {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }

    if padding.is_finite() {
        let pad = padding.max(0.0);
        minx -= pad;
        maxx += pad;
        miny -= pad;
        maxy += pad;
    }

    maxx >= min_cx && max_cx >= minx && maxy >= min_cy && max_cy >= miny
}

fn prepare_label_span(
    pts: &[Coord],
    total_length: f64,
    repeat_span: f64,
    label_start: f64,
    flip_needed: bool,
    trim_padding: f64,
    offset: f64,
    keep_offset_side: bool,
    clip_padding: f64,
    clip_extents: Option<(f64, f64, f64, f64)>,
) -> Option<PreparedLine> {
    // Orient the geometry according to the chosen upright direction.
    let mut oriented_pts = pts.to_vec();
    if flip_needed {
        oriented_pts.reverse();
    }
    let oriented_cum = cumulative_lengths(&oriented_pts);

    let start_use = if flip_needed {
        (total_length - repeat_span - label_start).max(0.0)
    } else {
        label_start
    };
    let span_end = start_use + repeat_span;

    let trim_start = (start_use - trim_padding).max(0.0);
    let trim_end = (span_end + trim_padding).min(total_length);

    let mut pts_use = trim_line_to_span(&oriented_pts, &oriented_cum, trim_start, trim_end);
    pts_use.dedup_by(|a, b| a == b);
    if pts_use.len() < 2 {
        return None;
    }

    // Offset only the trimmed slice to keep work bounded.
    if offset != 0.0 {
        let base_offset = -offset;
        let signed_offset = if flip_needed {
            if keep_offset_side {
                offset
            } else {
                base_offset
            }
        } else {
            base_offset
        };
        let ls = LineString::from(pts_use.clone());
        let offset_ls = offset_line_string(&ls, signed_offset);
        let mut off_pts: Vec<Coord> = offset_ls.into_iter().collect();
        off_pts.dedup_by(|a, b| a == b);
        if off_pts.len() < 2 {
            return None;
        }
        pts_use = off_pts;
    }

    if let Some(clip) = clip_extents
        && !bbox_intersects_clip(&pts_use, clip, clip_padding)
    {
        return None;
    }

    let cum_use = cumulative_lengths(&pts_use);
    let total_length_use = *cum_use.last().unwrap_or(&0.0);
    if total_length_use == 0.0 {
        return None;
    }

    let cursor_start = (start_use - trim_start).max(0.0);
    if cursor_start > total_length_use {
        return None;
    }

    Some(PreparedLine {
        pts: pts_use,
        cum: cum_use,
        total_length: total_length_use,
        cursor_start,
        trim_start,
    })
}

fn make_cluster_glyph_string(
    glyph_item: &GlyphItem,
    start_glyph: i32,
    end_glyph: i32,
) -> GlyphString {
    let src = glyph_item.glyph_string();
    let count = (end_glyph - start_glyph) as usize;
    let mut dst = GlyphString::new();
    dst.set_size(count as i32);

    for i in 0..count {
        let src_info = &src.glyph_info()[start_glyph as usize + i];
        let dst_info = &mut dst.glyph_info_mut()[i];

        dst_info.set_glyph(src_info.glyph());

        let src_geom = src_info.geometry();
        let dst_geom = dst_info.geometry_mut();

        dst_geom.set_width(src_geom.width());
        dst_geom.set_x_offset(src_geom.x_offset());
        dst_geom.set_y_offset(src_geom.y_offset());

        dst.log_clusters_mut()[i] = i as i32;
    }

    dst
}

fn collect_clusters(layout: &Layout) -> Vec<(f64, GlyphString, Font)> {
    let mut result = Vec::new();
    let ps = 1.0 / SCALE as f64;

    for line_idx in 0..layout.line_count() {
        let Some(line) = layout.line(line_idx) else {
            continue;
        };

        for run in line.runs() {
            let font = run.item().analysis().font();
            let glyphs = run.glyph_string();
            let infos = glyphs.glyph_info();
            let clusters = glyphs.log_clusters();

            if infos.is_empty() || clusters.is_empty() || infos.len() != clusters.len() {
                continue;
            }

            let mut start = 0usize;
            while start < infos.len() {
                let cluster_id = clusters[start];
                let mut end = start + 1;
                while end < infos.len() && clusters[end] == cluster_id {
                    end += 1;
                }

                let glyph_string = make_cluster_glyph_string(&run, start as i32, end as i32);
                let advance = glyph_string.glyph_info()[..]
                    .iter()
                    .map(|g| g.geometry().width() as f64 * ps)
                    .sum();

                result.push((advance, glyph_string, font.clone()));

                start = end;
            }
        }
    }

    result
}

fn draw_label(
    cr: &cairo::Context,
    glyphs: &[(GlyphString, Font, Coord, f64)],
    opts: &TextOnLineOptions,
) -> cairo::Result<()> {
    if glyphs.is_empty() {
        return Ok(());
    }

    let ps = 1.0 / SCALE as f64;

    cr.push_group();

    for (glyph_string, font, pos, angle) in glyphs {
        // Rotate around the glyph's centroid (center of logical bbox).
        let mut gs = glyph_string.clone();
        let (_, logical) = gs.extents(font);
        let cx = (logical.x() as f64 + logical.width() as f64 / 2.0) * ps;
        let cy = (logical.y() as f64 + logical.height() as f64 / 2.0) * ps;

        cr.save()?;
        cr.translate(pos.x, pos.y);
        cr.rotate(*angle);
        cr.translate(-cx, -cy);

        glyph_string_path(cr, font, &mut gs);

        cr.restore()?;
    }

    cr.set_source_color_a(opts.halo_color, opts.halo_opacity);
    cr.set_dash(&[], 0.0);
    cr.set_line_width(opts.halo_width * 2.0);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.stroke_preserve()?;

    cr.set_source_color(opts.color);
    cr.fill()?;

    cr.pop_group_to_source()?;
    cr.paint_with_alpha(opts.alpha)?;

    Ok(())
}

fn label_offsets(
    total_length: f64,
    label_span: f64,
    spacing: Option<f64>,
    align: Align,
) -> Vec<f64> {
    if total_length < label_span {
        return Vec::new();
    }

    // Step between label starts when repeating is enabled: pack by (advance + spacing).
    let step = spacing
        .map(|s| (label_span + s).max(label_span * 0.2))
        .unwrap_or(total_length);

    // How many full labels can we fit (repetition only if spacing is Some).
    let count = if spacing.is_some() {
        ((total_length - label_span) / step).floor() as usize + 1
    } else {
        1
    };

    let total_span = if count > 0 {
        step.mul_add((count.saturating_sub(1)) as f64, label_span)
    } else {
        0.0
    };

    let start = match align {
        Align::Left => 0.0,
        Align::Center => ((total_length - total_span) / 2.0).max(0.0),
        Align::Right => (total_length - total_span).max(0.0),
    };

    (0..count)
        .map(|i| (i as f64).mul_add(step, start))
        .collect()
}

fn justify_spacing(
    min_spacing: Option<f64>,
    total_length: f64,
    base_total_advance: f64,
    clusters: &[(f64, GlyphString, Font)],
) -> Option<(f64, f64)> {
    let gaps = clusters.len().saturating_sub(1) as f64;
    if gaps == 0.0 {
        return Some((1.0, 0.0));
    }

    let raw_extra = (total_length - base_total_advance) / gaps;
    let min_adv = clusters
        .iter()
        .map(|c| c.0)
        .fold(f64::INFINITY, f64::min)
        .max(0.0);

    // Allow slight compression (down to -80% of the narrowest advance), but keep spacing even.
    let min_gap = if min_adv.is_finite() {
        -min_adv * 0.8
    } else {
        raw_extra
    };

    let spacing = raw_extra.max(min_gap);
    if let Some(m) = min_spacing
        && spacing < m
    {
        return None;
    }

    Some((1.0, spacing))
}

fn center_offset_for_glyph(
    idx: usize,
    glyph_count: usize,
    eff_advance: f64,
    ink_left_rel: f64,
    ink_right_rel: f64,
) -> f64 {
    if glyph_count == 1 || idx == 0 {
        // Anchor the first glyph's ink left edge to the start of the span.
        -ink_left_rel
    } else if idx + 1 == glyph_count {
        // Pull the last glyph so its ink right edge sits on the span end.
        eff_advance - ink_right_rel
    } else {
        // Middle glyphs stay centered in their advance plus uniform gap.
        eff_advance / 2.0
    }
}

struct RepeatParams {
    span: f64,
    defer_collision: bool,
}

struct PreparedLine {
    pts: Vec<Coord>,
    cum: Vec<f64>,
    total_length: f64,
    cursor_start: f64,
    trim_start: f64,
}

fn repeat_params(
    spacing: Option<f64>,
    total_advance: f64,
    ink_span: f64,
    halo_width: f64,
) -> RepeatParams {
    if spacing.is_some() {
        RepeatParams {
            span: total_advance.max(halo_width.mul_add(2.0, ink_span)),
            defer_collision: true,
        }
    } else {
        RepeatParams {
            span: total_advance,
            defer_collision: false,
        }
    }
}

/// Draw text along a line. Returns `false` when Justify could not respect `min_spacing`.
pub fn draw_text_on_line(
    context: &Context,
    line_string: &LineString,
    text: &str,
    mut collision: Option<&mut Collision>,
    options: &TextOnLineOptions,
) -> cairo::Result<bool> {
    let _span = tracy_client::span!("text_on_line::draw_text_on_line");

    let ps = 1.0 / SCALE as f64;
    let mut pts: Vec<Coord> = line_string.into_iter().copied().collect();

    pts.dedup_by(|a, b| a == b);

    if pts.len() < 2 {
        return Ok(true);
    }

    let cum = cumulative_lengths(&pts);
    let total_length = *cum.last().unwrap_or(&0.0);

    if total_length == 0.0 {
        return Ok(true);
    }

    let clip_extents = context.clip_extents().ok();

    let TextOnLineOptions {
        distribution,
        upright,
        max_curvature_degrees,
        concave_spacing_factor,
        flo,
        offset,
        ..
    } = options;

    // Derive layout mode from distribution.
    let (align_mode, spacing_use, min_spacing) = match distribution {
        Distribution::Align { align, repeat } => {
            let spacing = match repeat {
                Repeat::None => None,
                Repeat::Spaced(s) => Some(*s),
            };
            (*align, spacing, None)
        }
        Distribution::Justify { min_spacing } => (Align::Left, None, *min_spacing),
    };
    let is_justify = min_spacing.is_some();
    let concave_spacing_factor = if is_justify {
        // Keep justification exact; extra curvature padding would shift glyphs off the span.
        0.0
    } else {
        *concave_spacing_factor
    };

    // For justify we ignore user letter spacing (scaling is applied instead).
    let flo_use = if min_spacing.is_some() {
        FontAndLayoutOptions {
            letter_spacing: 0.0,
            ..*flo
        }
    } else {
        *flo
    };

    let layout = create_layout_checked(context, "text-on-line", text, None, &flo_use, None)?;

    layout.set_width(-1); // no width constraint, so no wrapping happens at all
    let (ink, _) = layout.extents();
    let ink_span = (ink.width() as f64 / SCALE as f64).max(0.0);

    let clusters = collect_clusters(&layout);
    if clusters.is_empty() {
        return Ok(true);
    }

    let base_total_advance: f64 = clusters.iter().map(|c| c.0).sum();
    if base_total_advance == 0.0 {
        return Ok(true);
    }

    // If justify spacing falls below the configured minimum, abort drawing.
    let (advance_scale, extra_spacing_between_glyphs) = match min_spacing {
        Some(ms) => match justify_spacing(Some(ms), total_length, base_total_advance, &clusters) {
            Some(v) => v,
            None => return Ok(false),
        },
        None => (1.0, 0.0),
    };

    let total_advance = base_total_advance.mul_add(
        advance_scale,
        extra_spacing_between_glyphs * clusters.len().saturating_sub(1) as f64,
    );

    let repeat = repeat_params(spacing_use, total_advance, ink_span, options.halo_width);
    let offsets = if min_spacing.is_some() {
        vec![0.0]
    } else {
        label_offsets(total_length, repeat.span, spacing_use, align_mode)
    };
    let mut new_collision_bboxes: Vec<Rect<f64>> = Vec::new();

    if offsets.is_empty() {
        return Ok(false);
    }

    let mut placements: Vec<Vec<(GlyphString, Font, Coord, f64)>> = Vec::new();
    let mut rendered = false;

    // For each label repeat, walk glyphs along the line while keeping edge-alignment and curvature limits.
    'outer: for label_start in offsets {
        let mut label_start_try = label_start;
        let mut retries = 5usize;
        'attempt: loop {
            // Decide per-repeat if we need to flip to stay upright.
            let repeat_span = repeat.span;
            let overall_span_start = label_start_try;
            let overall_span_end = label_start_try + repeat_span;
            let overall_tangent =
                weighted_tangent_for_span(&pts, &cum, overall_span_start, overall_span_end)
                    .unwrap_or(Coord { x: 1.0, y: 0.0 });

            let base_angle = overall_tangent.y.atan2(overall_tangent.x);
            let adjusted_angle = adjust_upright_angle(base_angle, *upright);
            let flip_needed = (normalize_angle(adjusted_angle - base_angle)).abs() > PI / 2.0;
            let flip_offset = if flip_needed {
                0.0
            } else {
                normalize_angle(adjusted_angle - base_angle)
            };

            let trim_padding = (options.flo.size * 5.0) + options.halo_width + offset.abs();
            let keep_offset_side = options.keep_offset_side && matches!(upright, Upright::Auto);
            let clip_padding = options.halo_width + options.flo.size;
            let prepared = match prepare_label_span(
                &pts,
                total_length,
                repeat_span,
                label_start_try,
                flip_needed,
                trim_padding,
                *offset,
                keep_offset_side,
                clip_padding,
                clip_extents,
            ) {
                Some(p) => p,
                None => continue 'outer,
            };

            let mut cursor = prepared.cursor_start;
            let mut label_placements = Vec::new();
            let mut glyph_bboxes: Vec<Rect<f64>> = Vec::new();
            let mut glyph_span_ends: Vec<f64> = Vec::new();

            let label_advance_scale = advance_scale;
            let label_extra_spacing_between_glyphs = extra_spacing_between_glyphs;

            for (idx, (advance, glyph_string, font)) in clusters.iter().enumerate() {
                // Effective advance for this glyph (spacing between glyphs handled separately).
                let eff_advance = *advance * label_advance_scale;
                let span_start = cursor;
                let span_end = cursor + eff_advance;
                if span_end > prepared.total_length && !is_justify {
                    continue 'outer;
                }

                let (_, tangent) =
                    match position_at(&prepared.pts, &prepared.cum, span_start + eff_advance / 2.0)
                    {
                        Some(v) => v,
                        None => {
                            continue 'outer;
                        }
                    };

                let weighted_tangent =
                    weighted_tangent_for_span(&prepared.pts, &prepared.cum, span_start, span_end)
                        .unwrap_or(tangent);

                let tangent_before = position_at(&prepared.pts, &prepared.cum, span_start.max(0.0))
                    .map(|(_, t)| t)
                    .unwrap_or(weighted_tangent);

                let tangent_after = position_at(
                    &prepared.pts,
                    &prepared.cum,
                    span_end.min(prepared.total_length),
                )
                .map(|(_, t)| t)
                .unwrap_or(weighted_tangent);

                let mut max_bend = angle_between(tangent_before, tangent_after);

                for pair in
                    tangents_for_span(&prepared.pts, &prepared.cum, span_start, span_end).windows(2)
                {
                    max_bend = max_bend.max(angle_between(pair[0], pair[1]));
                }

                if max_bend > *max_curvature_degrees {
                    if retries == 0 {
                        continue 'outer;
                    }

                    retries -= 1;

                    // Skip a small distance past the bend and try again.
                    let bend_skip = (options.flo.size + options.halo_width).max(1.0);

                    let next_start_oriented =
                        (prepared.trim_start + span_end + bend_skip).min(total_length);

                    let next_label_start = if flip_needed {
                        (total_length - repeat_span - next_start_oriented).max(0.0)
                    } else {
                        next_start_oriented
                    };

                    if next_label_start + repeat_span <= total_length {
                        label_start_try = next_label_start;
                        continue 'attempt;
                    }

                    continue 'outer;
                }

                // Extra space proportional to curvature to avoid glyph tops touching on bends.
                let ratio = (max_bend / 180.0).clamp(0.0, 1.0);
                let concave_spacing = eff_advance * concave_spacing_factor * ratio;

                let shifted_start = span_start;
                let shifted_end = shifted_start + eff_advance;

                let mut gs_bbox = glyph_string.clone();
                let (ink, logical) = gs_bbox.extents(font);
                let logical_w = logical.width() as f64 * ps;
                let logical_h = logical.height() as f64 * ps;
                let ink_left = ink.x() as f64 * ps;
                let ink_right = (ink.x() as f64 + ink.width() as f64) * ps;
                let logical_cx = (logical.x() as f64 + logical.width() as f64 / 2.0) * ps;
                let ink_left_rel = ink_left - logical_cx;
                let ink_right_rel = ink_right - logical_cx;

                let center_offset = center_offset_for_glyph(
                    idx,
                    clusters.len(),
                    eff_advance,
                    ink_left_rel,
                    ink_right_rel,
                );

                let shifted_center = shifted_start + center_offset;

                if shifted_end > prepared.total_length && !is_justify {
                    continue 'outer;
                }

                let (pos, _) = match position_at(&prepared.pts, &prepared.cum, shifted_center) {
                    Some(v) => v,
                    None => {
                        continue 'outer;
                    }
                };

                let weighted_tangent = weighted_tangent_for_span(
                    &prepared.pts,
                    &prepared.cum,
                    shifted_start,
                    shifted_end,
                )
                .unwrap_or(weighted_tangent);

                let angle =
                    normalize_angle(weighted_tangent.y.atan2(weighted_tangent.x) + flip_offset);

                // Track an axis-aligned bbox for the rotated glyph.
                let hw = logical_w / 2.0;
                let hh = logical_h / 2.0;
                let cos = angle.cos().abs();
                let sin = angle.sin().abs();
                let rx = hw.mul_add(cos, hh * sin);
                let ry = hw.mul_add(sin, hh * cos);

                glyph_bboxes.push(Rect::new(
                    (pos.x - rx, pos.y - ry),
                    (pos.x + rx, pos.y + ry),
                ));
                glyph_span_ends.push(span_end);

                label_placements.push((glyph_string.clone(), font.clone(), pos, angle));

                cursor += eff_advance;

                if idx + 1 < clusters.len() {
                    cursor += concave_spacing + label_extra_spacing_between_glyphs;
                }
            }

            if let Some(col) = collision.as_deref()
                && let Some((idx, _)) = glyph_bboxes
                    .iter()
                    .enumerate()
                    .find(|(_, bb)| col.collides(bb))
            {
                if retries > 0 {
                    retries -= 1;
                    let skip = (options.halo_width + options.flo.size).max(1.0);

                    let collided_end_oriented =
                        prepared.trim_start + glyph_span_ends.get(idx).copied().unwrap_or(0.0);

                    let next_start_oriented = (collided_end_oriented + skip).min(total_length);

                    let next_label_start = if flip_needed {
                        (total_length - repeat_span - next_start_oriented).max(0.0)
                    } else {
                        next_start_oriented
                    };

                    if next_label_start + repeat_span <= total_length {
                        label_start_try = next_label_start;
                        continue 'attempt;
                    }
                }

                continue 'outer;
            }

            if repeat.defer_collision {
                new_collision_bboxes.extend(glyph_bboxes);
            } else if let Some(col) = collision.as_deref_mut() {
                for bb in glyph_bboxes {
                    let _ = col.add(bb);
                }
            }

            placements.push(label_placements);
            rendered = true;
            break 'attempt;
        }
    }

    if repeat.defer_collision
        && let Some(col) = collision
    {
        for bb in new_collision_bboxes.into_iter() {
            let _ = col.add(bb);
        }
    }

    for label in placements {
        draw_label(context, &label, options)?;
    }

    Ok(rendered)
}
