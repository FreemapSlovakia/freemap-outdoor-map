use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cairo::Context;
use std::rc::Rc;

use cosmic_text::{FontSystem, LayoutGlyph, fontdb};
use rustc_hash::FxHashMap;
use swash::scale::ScaleContext;
use swash::zeno::Verb;
use swash::{FontRef, scale::outline::Outline};

use crate::render::colors::{Color, ContextExt};
use geo::Rect;

/// Margin around a label's boxes for antialiasing at the halo's edge.
const HALO_CLIP_MARGIN: f64 = 2.0;

/// Fill and halo of a label.
pub struct HaloPaint {
    pub color: Color,
    pub halo_color: Color,
    pub halo_opacity: f64,
    pub halo_width: f64,
    pub alpha: f64,
}

/// Fills the path `build_path` makes over its halo, composited as one group at `alpha`.
///
/// `boxes` must cover the ink and halo: the group is clipped to them, as a group of the whole
/// clip costs a tile-sized buffer per label.
pub fn draw_with_halo(
    context: &Context,
    boxes: &[Rect<f64>],
    paint: &HaloPaint,
    build_path: impl FnOnce(),
) -> cairo::Result<()> {
    let Some(first) = boxes.first() else {
        return Ok(());
    };

    let (mut min, mut max) = (first.min(), first.max());

    for bb in &boxes[1..] {
        min.x = min.x.min(bb.min().x);
        min.y = min.y.min(bb.min().y);
        max.x = max.x.max(bb.max().x);
        max.y = max.y.max(bb.max().y);
    }

    context.save()?;

    // A path left by the caller would otherwise join the clip.
    context.new_path();

    context.rectangle(
        min.x - HALO_CLIP_MARGIN,
        min.y - HALO_CLIP_MARGIN,
        HALO_CLIP_MARGIN.mul_add(2.0, max.x - min.x),
        HALO_CLIP_MARGIN.mul_add(2.0, max.y - min.y),
    );

    context.clip();

    build_path();

    context.push_group();

    context.set_source_color_a(paint.halo_color, paint.halo_opacity);
    context.set_dash(&[], 0.0);
    context.set_line_join(cairo::LineJoin::Round);
    context.set_line_width(paint.halo_width * 2.0);
    context.stroke_preserve()?;

    context.set_source_color(paint.color);
    context.fill()?;

    context.pop_group_to_source()?;
    context.paint_with_alpha(paint.alpha)?;

    context.restore()
}

static FONTS_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn set_fonts_path(path: PathBuf) {
    FONTS_PATH
        .set(path)
        .expect("fonts path already configured");
}

fn configured_fonts_path() -> &'static Path {
    FONTS_PATH
        .get()
        .map(PathBuf::as_path)
        .expect("fonts path not configured; call set_fonts_path() at startup")
}

fn build_font_system(fonts_dir: &Path) -> FontSystem {
    let mut db = fontdb::Database::new();
    db.load_fonts_dir(fonts_dir);
    FontSystem::new_with_locale_and_db("en-US".to_string(), db)
}

thread_local! {
    static FONT_SYSTEM: RefCell<FontSystem> =
        RefCell::new(build_font_system(configured_fonts_path()));
    static SCALE_CONTEXT: RefCell<ScaleContext> = RefCell::new(ScaleContext::new());
    static OUTLINE_CACHE: RefCell<FxHashMap<OutlineKey, Option<Rc<Outline>>>> =
        RefCell::new(FxHashMap::default());
}

/// Glyph outlines kept per render thread; swash rebuilds an outline on every scale.
const OUTLINE_CACHE_CAPACITY: usize = 8192;

/// Font, weight, size bits and glyph id.
type OutlineKey = (fontdb::ID, fontdb::Weight, u32, u16);

pub fn with_font_system<R>(f: impl FnOnce(&mut FontSystem) -> R) -> R {
    FONT_SYSTEM.with(|fs| f(&mut fs.borrow_mut()))
}

pub fn with_scale_context<R>(f: impl FnOnce(&mut ScaleContext) -> R) -> R {
    SCALE_CONTEXT.with(|sc| f(&mut sc.borrow_mut()))
}

/// Scale `glyph_id` at `font_size` and return the outline, or `None` for
/// glyphs without outlines (bitmap-only, whitespace, etc.).
pub fn scale_outline(
    scale_ctx: &mut ScaleContext,
    font_ref: FontRef<'_>,
    font_size: f32,
    glyph_id: u16,
) -> Option<Outline> {
    scale_ctx
        .builder(font_ref)
        .size(font_size)
        .build()
        .scale_outline(glyph_id)
}

/// Scaled outline of a shaped glyph, cached; `None` when its font is missing or it has no
/// outline.
pub fn glyph_outline(
    font_system: &mut FontSystem,
    scale_ctx: &mut ScaleContext,
    glyph: &LayoutGlyph,
) -> Option<Rc<Outline>> {
    let key = (
        glyph.font_id,
        glyph.font_weight,
        glyph.font_size.to_bits(),
        glyph.glyph_id,
    );

    OUTLINE_CACHE.with(|cache| {
        if let Some(outline) = cache.borrow().get(&key) {
            return outline.clone();
        }

        let outline = font_system
            .get_font(glyph.font_id, glyph.font_weight)
            .and_then(|font| {
                scale_outline(scale_ctx, font.as_swash(), glyph.font_size, glyph.glyph_id)
            })
            .map(Rc::new);

        let mut cache = cache.borrow_mut();

        if cache.len() >= OUTLINE_CACHE_CAPACITY {
            cache.clear();
        }

        cache.insert(key, outline.clone());

        outline
    })
}

/// Emit an already-scaled `outline`'s path to `context` translated to
/// `(gx, gy)` with Y flipped (font outlines are Y-up from the baseline,
/// cairo is Y-down). Quadratic beziers are converted to cubics so cairo
/// can draw them. Does not open a new path or stroke/fill.
pub fn stamp_outline(context: &Context, outline: &Outline, gx: f64, gy: f64) {
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
                let c1x = (2.0_f64 / 3.0).mul_add(x1 - x0, x0);
                let c1y = (2.0_f64 / 3.0).mul_add(y1 - y0, y0);
                let c2x = (2.0_f64 / 3.0).mul_add(x1 - x2, x2);
                let c2y = (2.0_f64 / 3.0).mul_add(y1 - y2, y2);
                context.curve_to(c1x, c1y, c2x, c2y, x2, y2);
                cur = (x2, y2);
            }
            Verb::Close => {
                context.close_path();
            }
        }
    }
}
