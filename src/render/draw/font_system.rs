use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cairo::Context;
use cosmic_text::{FontSystem, fontdb};
use swash::scale::ScaleContext;
use swash::zeno::Verb;
use swash::{FontRef, scale::outline::Outline};

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
}

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
