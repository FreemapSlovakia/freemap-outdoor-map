use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cosmic_text::{FontSystem, fontdb};
use swash::scale::ScaleContext;

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
