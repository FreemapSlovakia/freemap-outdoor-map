use crate::render::{
    collision::Collision,
    colors::{self, Color},
    draw::{
        font_options::{FontAndLayoutOptions, ShapeKey, label_attrs, label_text},
        font_system::{
            HaloPaint, draw_with_halo, glyph_outline, stamp_outline, with_font_system,
            with_scale_context,
        },
        offset_line::offset_line_string,
        path_geom::cumulative_lengths,
    },
};
use cairo::Context;
use cosmic_text::{Buffer, Metrics, Shaping, Wrap};
use geo::{Coord, Euclidean, InterpolatePoint, LineString, Rect, Vector2DOps};
use rustc_hash::FxHashMap;
use std::{
    cell::{OnceCell, RefCell},
    f64::consts::{FRAC_PI_2, PI, TAU},
    rc::Rc,
};
use swash::scale::outline::Outline;

/// Vetoes a placement. The repeat is dropped rather than slid, so the decision rests on the
/// label's own box, which neighbouring tiles agree on.
pub type PlacementFilter<'a> = &'a dyn Fn(&Rect<f64>) -> bool;

#[derive(Copy, Clone)]
pub struct TextOnLineOptions<'a> {
    pub upright: Upright,
    pub distribution: Distribution,
    pub placement_filter: Option<PlacementFilter<'a>>,
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

impl Default for TextOnLineOptions<'_> {
    fn default() -> Self {
        Self {
            upright: Upright::Auto,
            placement_filter: None,
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

impl From<&TextOnLineOptions<'_>> for HaloPaint {
    fn from(options: &TextOnLineOptions<'_>) -> Self {
        Self {
            color: options.color,
            halo_color: options.halo_color,
            halo_opacity: options.halo_opacity,
            halo_width: options.halo_width,
            alpha: options.alpha,
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
    Justify { min_spacing: f64 },
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

/// Maps a span start between the forward and the reversed line (either way).
fn flip_start(total_length: f64, span: f64, start: f64) -> f64 {
    (total_length - span - start).max(0.0)
}

/// Indices of the segments that may overlap `span_start..span_end`: segments ending at or
/// before the span or starting at or after it can't.
fn segment_range(cum: &[f64], span_start: f64, span_end: f64) -> (usize, usize) {
    let first = cum[1..].partition_point(|&c| c <= span_start);
    let last = cum[..cum.len() - 1].partition_point(|&c| c < span_end);

    (first, last.max(first))
}

/// Segments overlapping `span_start..span_end`, as (unit tangent, overlap length).
fn span_segments(
    pts: &[Coord],
    cum: &[f64],
    span_start: f64,
    span_end: f64,
) -> impl Iterator<Item = (Coord, f64)> {
    let (first, last) = segment_range(cum, span_start, span_end);

    pts[first..=last]
        .windows(2)
        .zip(cum[first..=last].windows(2))
        .filter_map(move |(p, c)| {
            let overlap = span_end.min(c[1]) - span_start.max(c[0]);

            (overlap > 0.0).then(|| (normalize(p[1] - p[0]), overlap))
        })
}

fn weighted_tangent_for_span(
    pts: &[Coord],
    cum: &[f64],
    span_start: f64,
    span_end: f64,
) -> Option<Coord> {
    let mut accum = Coord { x: 0.0, y: 0.0 };
    let mut total = 0.0;

    for (tangent, weight) in span_segments(pts, cum, span_start, span_end) {
        accum = accum + tangent * weight;
        total += weight;
    }

    (total != 0.0).then(|| normalize(accum))
}

/// [`weighted_tangent_for_span`] and the largest turn between consecutive segments, in degrees,
/// in one pass.
fn weighted_tangent_and_turn(
    pts: &[Coord],
    cum: &[f64],
    span_start: f64,
    span_end: f64,
) -> (Option<Coord>, Option<f64>) {
    let mut accum = Coord { x: 0.0, y: 0.0 };
    let mut total = 0.0;
    let mut prev_tangent = None;
    let mut turn: Option<f64> = None;

    for (tangent, weight) in span_segments(pts, cum, span_start, span_end) {
        accum = accum + tangent * weight;
        total += weight;

        if let Some(prev) = prev_tangent {
            let angle = angle_between(prev, tangent);
            turn = Some(turn.map_or(angle, |turn| turn.max(angle)));
        }

        prev_tangent = Some(tangent);
    }

    ((total != 0.0).then(|| normalize(accum)), turn)
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

    let idx = cum[1..].partition_point(|&c| c < dist);

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

    let (first, last) = segment_range(cum, start, end);

    let mut trimmed = Vec::with_capacity(last - first + 2);

    if let Some((p, _)) = position_at(pts, cum, start) {
        trimmed.push(p);
    }

    trimmed.extend_from_slice(&pts[first + 1..=last]);

    if let Some((p, _)) = position_at(pts, cum, end)
        && trimmed.last().is_none_or(|q| *q != p)
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

/// One glyph within a cluster, with its offset from the cluster origin
/// (at the baseline, advance-aligned).
struct GlyphSpec {
    /// Scaled outline, `None` for glyphs without one (e.g. spaces).
    outline: Option<Rc<Outline>>,
    /// Offset of this glyph's pen position from the cluster origin.
    dx: f32,
    /// Same for vertical (usually 0 except for stacking diacritics).
    dy: f32,
}

/// A pango-style "cluster" = a run of glyphs that render together (e.g. a
/// base letter plus combining marks). Positioned as a unit.
struct ClusterInfo {
    /// Total horizontal advance (width the next cluster's origin sits at).
    advance: f64,
    /// Ink extents relative to the cluster origin; `y` axis is layout Y-down,
    /// so `ink_top` is negative (above baseline) and `ink_bottom` may be
    /// positive (below baseline).
    ink_left: f64,
    ink_right: f64,
    ink_top: f64,
    ink_bottom: f64,
    /// Logical (metric-ascent+descent) extents, used to size the collision
    /// bbox after rotation. Origin-relative, same Y convention as ink.
    logical_left: f64,
    logical_right: f64,
    logical_top: f64,
    logical_bottom: f64,
    glyphs: Vec<GlyphSpec>,
}

impl ClusterInfo {
    const fn logical_center(&self) -> (f64, f64) {
        (
            f64::midpoint(self.logical_left, self.logical_right),
            f64::midpoint(self.logical_top, self.logical_bottom),
        )
    }
}

/// Shape `text` with `flo` into a single unwrapped line and walk its glyphs,
/// grouping consecutive glyphs sharing `glyph.start` into one cluster.
/// Cluster positions, ink extents, and logical extents are all relative to
/// the cluster's pen origin (at the baseline).
fn collect_clusters(text: &str, flo: &FontAndLayoutOptions) -> Vec<ClusterInfo> {
    let attrs = label_attrs(flo);

    let text = label_text(text, flo);

    let size = flo.size as f32;
    let metrics = Metrics::new(size, size);

    with_font_system(|fs| {
        let mut buffer = Buffer::new(fs, metrics);
        buffer.set_wrap(Wrap::None);
        {
            let mut buf = buffer.borrow_with(fs);
            buf.set_size(None, None);
            buf.set_text(&text, &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(true);
        }

        with_scale_context(|sc| {
            let mut out: Vec<ClusterInfo> = Vec::new();
            // Track, for the currently-open cluster, the byte-range `start`
            // and the pen x at which the cluster began. Combining glyphs
            // share a `start` and get merged; otherwise we open a new one.
            let mut open: Option<(usize, f64)> = None;

            for run in buffer.layout_runs() {
                for glyph in run.glyphs {
                    let Some(font) = fs.get_font(glyph.font_id, glyph.font_weight) else {
                        continue;
                    };

                    let fm = font.as_swash().metrics(&[]);
                    let s = glyph.font_size / fm.units_per_em as f32;
                    let g_asc = (fm.ascent * s) as f64;
                    let g_desc = (fm.descent.abs() * s) as f64;

                    // Per-glyph ink box in the glyph's own pen coords (Y-up → Y-down flip).
                    let outline = glyph_outline(fs, sc, glyph);

                    let (g_ink_l, g_ink_r, g_ink_t, g_ink_b) =
                        outline.as_ref().map_or((0.0, 0.0, 0.0, 0.0), |o| {
                            let b = o.bounds();
                            (
                                b.min.x as f64,
                                b.max.x as f64,
                                -(b.max.y as f64),
                                -(b.min.y as f64),
                            )
                        });

                    let gx = glyph.x as f64;

                    let spec = GlyphSpec {
                        outline,
                        dx: 0.0,
                        dy: glyph.y,
                    };

                    if let Some((start, origin_x)) = open
                        && start == glyph.start
                        && let Some(cluster) = out.last_mut()
                    {
                        let rel_x = gx - origin_x;
                        // Glyph box in cluster-origin coords.
                        let l = rel_x + g_ink_l;
                        let r = rel_x + g_ink_r;
                        cluster.advance += glyph.w as f64;
                        cluster.ink_left = cluster.ink_left.min(l);
                        cluster.ink_right = cluster.ink_right.max(r);
                        cluster.ink_top = cluster.ink_top.min(g_ink_t);
                        cluster.ink_bottom = cluster.ink_bottom.max(g_ink_b);
                        cluster.logical_right = cluster.logical_right.max(rel_x + glyph.w as f64);
                        cluster.logical_top = cluster.logical_top.min(-g_asc);
                        cluster.logical_bottom = cluster.logical_bottom.max(g_desc);
                        cluster.glyphs.push(GlyphSpec {
                            dx: rel_x as f32,
                            ..spec
                        });
                    } else {
                        open = Some((glyph.start, gx));
                        out.push(ClusterInfo {
                            advance: glyph.w as f64,
                            ink_left: g_ink_l,
                            ink_right: g_ink_r,
                            ink_top: g_ink_t,
                            ink_bottom: g_ink_b,
                            logical_left: 0.0,
                            logical_right: glyph.w as f64,
                            logical_top: -g_asc,
                            logical_bottom: g_desc,
                            glyphs: vec![spec],
                        });
                    }
                }
            }

            out
        })
    })
}

/// Bounds memory per render thread; outlines are shared, so an entry takes a few kilobytes.
const SHAPED_CACHE_CAPACITY: usize = 2048;

thread_local! {
    static SHAPED_CACHE: RefCell<FxHashMap<ShapeKey, Rc<[ClusterInfo]>>> =
        RefCell::new(FxHashMap::default());
}

/// [`collect_clusters`], cached: the same names, refs and contour heights recur across a tile's
/// lines and neighbouring tiles.
fn shaped_clusters(text: &str, flo: &FontAndLayoutOptions) -> Rc<[ClusterInfo]> {
    let key = ShapeKey::new(text, flo);

    if let Some(clusters) = SHAPED_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return clusters;
    }

    let clusters: Rc<[ClusterInfo]> = collect_clusters(text, flo).into();

    SHAPED_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();

        if cache.len() >= SHAPED_CACHE_CAPACITY {
            cache.clear();
        }

        cache.insert(key, Rc::clone(&clusters));
    });

    clusters
}

fn draw_label(
    cr: &cairo::Context,
    laid_out: &LaidOut,
    opts: &TextOnLineOptions,
) -> cairo::Result<()> {
    draw_with_halo(cr, &laid_out.bboxes, &HaloPaint::from(opts), || {
        for (cluster, pos, angle) in &laid_out.glyphs {
            // Rotate around the cluster's logical bbox center.
            let (cx, cy) = cluster.logical_center();

            cr.save().ok();
            cr.translate(pos.x, pos.y);
            cr.rotate(*angle);
            cr.translate(-cx, -cy);

            for g in &cluster.glyphs {
                if let Some(outline) = &g.outline {
                    stamp_outline(cr, outline, g.dx as f64, g.dy as f64);
                }
            }

            cr.restore().ok();
        }
    })
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
        .map_or(total_length, |s| (label_span + s).max(label_span * 0.2));

    // How many full labels can we fit (repetition only if spacing is Some).
    let count = if spacing.is_some() {
        ((total_length - label_span) / step).floor() as usize + 1
    } else {
        1
    };

    let total_span = step.mul_add((count - 1) as f64, label_span);

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
    min_spacing: f64,
    total_length: f64,
    ink_span: f64,
    clusters: &[ClusterInfo],
) -> Option<f64> {
    let gaps = clusters.len().saturating_sub(1) as f64;
    if gaps == 0.0 {
        return Some(0.0);
    }

    let raw_extra = (total_length - ink_span) / gaps;
    let min_adv = clusters
        .iter()
        .map(|c| c.advance)
        .fold(f64::INFINITY, f64::min)
        .max(0.0);

    // Allow slight compression (down to -80% of the narrowest advance), but keep spacing even.
    let spacing = raw_extra.max(-min_adv * 0.8);

    if spacing < min_spacing {
        None
    } else {
        Some(spacing)
    }
}

/// Leftmost ink position along pen-x, and the distance from leftmost to rightmost ink.
fn ink_extents(clusters: &[ClusterInfo]) -> (f64, f64) {
    let mut cum = 0.0_f64;
    let mut min_l = f64::INFINITY;
    let mut max_r = f64::NEG_INFINITY;

    for c in clusters {
        min_l = min_l.min(cum + c.ink_left);
        max_r = max_r.max(cum + c.ink_right);
        cum += c.advance;
    }

    (min_l, (max_r - min_l).max(0.0))
}

/// Axis-aligned bbox of the cluster's ink rectangle, inflated by `halo_width` on every side
/// before rotation so the halo rotates with the glyph. `pos` is where the cluster's logical
/// center lands on screen.
fn rotated_ink_bbox(cluster: &ClusterInfo, pos: Coord, angle: f64, halo_width: f64) -> Rect<f64> {
    let (cx, cy) = cluster.logical_center();
    let hw = halo_width;
    let corners = [
        (cluster.ink_left - hw - cx, cluster.ink_top - hw - cy),
        (cluster.ink_right + hw - cx, cluster.ink_top - hw - cy),
        (cluster.ink_right + hw - cx, cluster.ink_bottom + hw - cy),
        (cluster.ink_left - hw - cx, cluster.ink_bottom + hw - cy),
    ];
    let c = angle.cos();
    let s = angle.sin();
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;

    for (dx, dy) in corners {
        let rx = dy.mul_add(-s, dx * c);
        let ry = dy.mul_add(c, dx * s);
        minx = minx.min(rx);
        miny = miny.min(ry);
        maxx = maxx.max(rx);
        maxy = maxy.max(ry);
    }

    Rect::new((pos.x + minx, pos.y + miny), (pos.x + maxx, pos.y + maxy))
}

struct PreparedLine {
    pts: Vec<Coord>,
    cum: Vec<f64>,
    total_length: f64,
    cursor_start: f64,
    trim_start: f64,
    intersects_clip: bool,
}

/// The label's line, measured once per call.
struct Line {
    pts: Vec<Coord>,
    cum: Vec<f64>,
    total_length: f64,
    /// Built on the first flip; many lines never flip.
    reversed: OnceCell<(Vec<Coord>, Vec<f64>)>,
}

impl Line {
    fn oriented(&self, reversed: bool) -> (&[Coord], &[f64]) {
        if reversed {
            let (pts, cum) = self.reversed.get_or_init(|| {
                let pts: Vec<Coord> = self.pts.iter().rev().copied().collect();
                let cum = cumulative_lengths(&pts);
                (pts, cum)
            });

            (pts, cum)
        } else {
            (&self.pts, &self.cum)
        }
    }
}

/// Glyphs of one repeat laid along a prepared line.
struct LaidOut<'a> {
    bboxes: Vec<Rect<f64>>,
    span_ends: Vec<f64>,
    /// Empty when the repeat is outside the clip.
    glyphs: Vec<(&'a ClusterInfo, Coord, f64)>,
}

enum Rejection {
    Drop,
    /// The glyph ending at `span_end` on the prepared line sits on too sharp a bend.
    Bend { span_end: f64 },
}

const MAX_RETRIES: usize = 5;

/// Places single repeats of a label, sliding them past sharp bends and collisions.
struct RepeatPlacer<'a> {
    line: &'a Line,
    options: &'a TextOnLineOptions<'a>,
    clusters: &'a [ClusterInfo],
    ink_lead: f64,
    extra_spacing_between_glyphs: f64,
    concave_spacing_factor: f64,
    allow_overflow: bool,
    repeat_span: f64,
    clip_extents: Option<(f64, f64, f64, f64)>,
}

impl<'a> RepeatPlacer<'a> {
    /// Where the repeat finally starts and its glyphs, or `None` when it is dropped.
    fn place(&self, first_start: f64, collision: &Collision) -> Option<(f64, LaidOut<'a>)> {
        let mut label_start = first_start;
        let mut retries = MAX_RETRIES;

        loop {
            let flip_needed = self.flip_needed(label_start);

            let prepared = self.prepare(label_start, flip_needed)?;

            let retry_from = match self.lay_out(&prepared) {
                Err(Rejection::Drop) => return None,
                Err(Rejection::Bend { span_end }) => span_end,
                Ok(laid_out) => {
                    if self
                        .options
                        .placement_filter
                        .is_some_and(|allows| laid_out.bboxes.iter().any(|bb| !allows(bb)))
                    {
                        return None;
                    }

                    match laid_out
                        .bboxes
                        .iter()
                        .position(|bb| collision.collides(bb, None))
                    {
                        None => return Some((label_start, laid_out)),
                        Some(idx) => laid_out.span_ends[idx],
                    }
                }
            };

            if retries == 0 {
                return None;
            }

            retries -= 1;
            label_start = self.retry_start(&prepared, flip_needed, retry_from)?;
        }
    }

    /// Whether to lay the label along the reversed line so it reads upright.
    fn flip_needed(&self, label_start: f64) -> bool {
        match self.options.upright {
            Upright::Left => true,
            Upright::Right => false,
            Upright::Auto => {
                let tangent = weighted_tangent_for_span(
                    &self.line.pts,
                    &self.line.cum,
                    label_start,
                    label_start + self.repeat_span,
                )
                .unwrap_or(Coord { x: 1.0, y: 0.0 });

                tangent.y.atan2(tangent.x).abs() > FRAC_PI_2
            }
        }
    }

    /// Trims (and offsets) the line around a repeat. `label_start` is along the forward line
    /// even when `flip_needed`.
    fn prepare(&self, label_start: f64, flip_needed: bool) -> Option<PreparedLine> {
        let options = self.options;
        let total_length = self.line.total_length;
        let (oriented_pts, oriented_cum) = self.line.oriented(flip_needed);

        let start_use = if flip_needed {
            flip_start(total_length, self.repeat_span, label_start)
        } else {
            label_start
        };
        let span_end = start_use + self.repeat_span;

        let trim_padding = options.flo.size.mul_add(5.0, options.halo_width) + options.offset.abs();
        let trim_start = (start_use - trim_padding).max(0.0);
        let trim_end = (span_end + trim_padding).min(total_length);

        let mut pts_use = trim_line_to_span(oriented_pts, oriented_cum, trim_start, trim_end);
        pts_use.dedup();
        if pts_use.len() < 2 {
            return None;
        }

        // Offset only the trimmed slice to keep work bounded.
        let pts_use = if options.offset == 0.0 {
            pts_use
        } else {
            let keep_offset_side =
                options.keep_offset_side && matches!(options.upright, Upright::Auto);

            let signed_offset = if flip_needed && keep_offset_side {
                options.offset
            } else {
                -options.offset
            };

            let mut off_pts: Vec<Coord> =
                offset_line_string(&LineString::from(pts_use), signed_offset)
                    .into_iter()
                    .collect();

            off_pts.dedup();
            if off_pts.len() < 2 {
                return None;
            }

            off_pts
        };

        let clip_padding = options.halo_width + options.flo.size;
        let intersects_clip = self
            .clip_extents
            .is_none_or(|clip| bbox_intersects_clip(&pts_use, clip, clip_padding));

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
            intersects_clip,
        })
    }

    fn lay_out(&self, prepared: &PreparedLine) -> Result<LaidOut<'a>, Rejection> {
        let clusters = self.clusters;

        let mut laid_out = LaidOut {
            bboxes: Vec::with_capacity(clusters.len()),
            span_ends: Vec::with_capacity(clusters.len()),
            glyphs: if prepared.intersects_clip {
                Vec::with_capacity(clusters.len())
            } else {
                Vec::new()
            },
        };

        // Shift the whole label so its leftmost ink lands at the span
        // start. This keeps every inter-glyph gap uniform (just the
        // natural side-bearings), instead of pulling the first/last
        // glyphs inward to anchor their ink edges.
        let mut cursor = prepared.cursor_start - self.ink_lead;

        for (idx, cluster) in clusters.iter().enumerate() {
            let span_start = cursor;
            let span_end = cursor + cluster.advance;
            if span_end > prepared.total_length && !self.allow_overflow {
                return Err(Rejection::Drop);
            }

            let Some((pos, tangent)) = position_at(
                &prepared.pts,
                &prepared.cum,
                span_start + cluster.advance / 2.0,
            ) else {
                return Err(Rejection::Drop);
            };

            let (weighted_tangent, turn) =
                weighted_tangent_and_turn(&prepared.pts, &prepared.cum, span_start, span_end);

            let weighted_tangent = weighted_tangent.unwrap_or(tangent);

            let tangent_before = position_at(&prepared.pts, &prepared.cum, span_start.max(0.0))
                .map_or(weighted_tangent, |(_, t)| t);

            let tangent_after = position_at(
                &prepared.pts,
                &prepared.cum,
                span_end.min(prepared.total_length),
            )
            .map_or(weighted_tangent, |(_, t)| t);

            // Largest turn of the line under the glyph, in degrees.
            let ends_turn = angle_between(tangent_before, tangent_after);
            let bend = turn.map_or(ends_turn, |turn| ends_turn.max(turn));

            if bend > self.options.max_curvature_degrees {
                return Err(Rejection::Bend { span_end });
            }

            // Extra space proportional to curvature to avoid glyph tops touching on bends.
            let ratio = (bend / 180.0).clamp(0.0, 1.0);
            let concave_spacing = cluster.advance * self.concave_spacing_factor * ratio;

            let angle = normalize_angle(weighted_tangent.y.atan2(weighted_tangent.x));

            laid_out.bboxes.push(rotated_ink_bbox(
                cluster,
                pos,
                angle,
                self.options.halo_width,
            ));

            laid_out.span_ends.push(span_end);

            if prepared.intersects_clip {
                laid_out.glyphs.push((cluster, pos, angle));
            }

            cursor += cluster.advance;

            if idx + 1 < clusters.len() {
                cursor += concave_spacing + self.extra_spacing_between_glyphs;
            }
        }

        Ok(laid_out)
    }

    /// Label start for a retry just past `oriented_end`, a glyph end on the prepared line.
    fn retry_start(
        &self,
        prepared: &PreparedLine,
        flip_needed: bool,
        oriented_end: f64,
    ) -> Option<f64> {
        let total_length = self.line.total_length;
        let retry_skip = (self.options.halo_width + self.options.flo.size).max(1.0);

        let next = (prepared.trim_start + oriented_end + retry_skip).min(total_length);

        let next = if flip_needed {
            flip_start(total_length, self.repeat_span, next)
        } else {
            next
        };

        (next + self.repeat_span <= total_length).then_some(next)
    }
}

/// Draw text along a line. Returns `false` when Justify could not respect `min_spacing`.
pub fn draw_text_on_line(
    context: &Context,
    line_string: &LineString,
    text: &str,
    collision: Option<&mut Collision>,
    options: &TextOnLineOptions,
) -> cairo::Result<bool> {
    let _span = tracy_client::span!("text_on_line::draw_text_on_line");

    let mut pts: Vec<Coord> = line_string.into_iter().copied().collect();

    pts.dedup();

    if pts.len() < 2 {
        return Ok(true);
    }

    let cum = cumulative_lengths(&pts);
    let total_length = *cum.last().unwrap_or(&0.0);

    if total_length == 0.0 {
        return Ok(true);
    }

    let (align, spacing, justify_min_spacing) = match options.distribution {
        Distribution::Align { align, repeat } => {
            let spacing = match repeat {
                Repeat::None => None,
                Repeat::Spaced(s) => Some(s),
            };
            (align, spacing, None)
        }
        Distribution::Justify { min_spacing } => (Align::Left, None, Some(min_spacing)),
    };

    let is_justify = justify_min_spacing.is_some();

    // Justify sets its own spacing between glyphs.
    let flo = if is_justify {
        FontAndLayoutOptions {
            letter_spacing: 0.0,
            ..options.flo
        }
    } else {
        options.flo
    };

    let clusters = shaped_clusters(text, &flo);
    if clusters.is_empty() {
        return Ok(true);
    }

    let (ink_lead, ink_span) = ink_extents(&clusters);

    if ink_span == 0.0 {
        return Ok(true);
    }

    // If justify spacing falls below the configured minimum, abort drawing.
    let extra_spacing_between_glyphs = match justify_min_spacing {
        Some(ms) => {
            let Some(spacing) = justify_spacing(ms, total_length, ink_span, &clusters) else {
                return Ok(false);
            };
            spacing
        }
        None => 0.0,
    };

    let extra_width = extra_spacing_between_glyphs * clusters.len().saturating_sub(1) as f64;
    let label_visual_span = ink_span + extra_width;

    let repeat_span = if spacing.is_some() {
        label_visual_span.max(options.halo_width.mul_add(2.0, ink_span))
    } else {
        label_visual_span
    };

    let offsets = if is_justify {
        vec![0.0]
    } else {
        label_offsets(total_length, repeat_span, spacing, align)
    };

    if offsets.is_empty() {
        return Ok(false);
    }

    let line = Line {
        pts,
        cum,
        total_length,
        reversed: OnceCell::new(),
    };

    let placer = RepeatPlacer {
        line: &line,
        options,
        clusters: &clusters,
        ink_lead,
        extra_spacing_between_glyphs,
        // Keep justification exact; extra curvature padding would shift glyphs off the span.
        concave_spacing_factor: if is_justify {
            0.0
        } else {
            options.concave_spacing_factor
        },
        allow_overflow: is_justify,
        repeat_span,
        clip_extents: context.clip_extents().ok(),
    };

    // Repeats must avoid each other even when the caller keeps no collision set.
    let mut local_collision = Collision::new(None);
    let collision = collision.unwrap_or(&mut local_collision);

    let mut placements = Vec::new();

    // Retries shift a repeat along the line; start the next one a full spacing after it.
    let mut min_label_start = 0.0;

    for label_start in offsets {
        let Some((start, laid_out)) = placer.place(label_start.max(min_label_start), collision)
        else {
            continue;
        };

        min_label_start = start + repeat_span + spacing.unwrap_or(0.0);

        // Added right away: a retry can slide a repeat onto the next one, and a line folding
        // back (switchbacks) brings distant repeats close on the map.
        for bb in &laid_out.bboxes {
            let _ = collision.add(*bb);
        }

        if !laid_out.glyphs.is_empty() {
            placements.push(laid_out);
        }
    }

    let rendered = !placements.is_empty();

    for laid_out in &placements {
        draw_label(context, laid_out, options)?;
    }

    Ok(rendered)
}
