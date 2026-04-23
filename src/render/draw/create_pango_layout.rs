use cairo::Context;
use pangocairo::pango;
use pangocairo::prelude::FontMapExt;
use pangocairo::{
    FontMap,
    functions::{context_set_font_options, context_set_resolution, update_context, update_layout},
    pango::{
        Alignment, AttrInt, AttrList, FontDescription, Layout, SCALE, Style, Weight, WrapMode,
    },
};
use std::borrow::Cow;
use std::sync::LazyLock;

thread_local! {
    static PANGO_FONT_MAP: pango::FontMap = FontMap::new();
}

static CANARY: LazyLock<String> =
    LazyLock::new(|| std::env::var("MAPRENDER_TOFU_CANARY").unwrap_or_else(|_| "A".to_string()));

#[derive(Copy, Clone, Debug)]
pub struct FontAndLayoutOptions {
    pub letter_spacing: f64,
    pub max_width: f64,
    pub narrow: bool,
    pub size: f64,
    pub style: Style,
    pub uppercase: bool,
    pub weight: Weight,
}

impl Default for FontAndLayoutOptions {
    fn default() -> Self {
        Self {
            letter_spacing: 0.0,
            max_width: 100.0,
            narrow: false,
            size: 12.0,
            style: Style::Normal,
            uppercase: false,
            weight: Weight::Normal,
        }
    }
}

impl FontAndLayoutOptions {
    /// Numeric CSS-style weight (100–1000) corresponding to `self.weight`.
    /// Used to drive cosmic-text / fontdb font matching without pulling pango
    /// types into downstream modules.
    pub fn ct_weight_u16(&self) -> u16 {
        match self.weight {
            Weight::Thin => 100,
            Weight::Ultralight => 200,
            Weight::Light => 300,
            Weight::Semilight => 350,
            Weight::Book => 380,
            Weight::Normal => 400,
            Weight::Medium => 500,
            Weight::Semibold => 600,
            Weight::Bold => 700,
            Weight::Ultrabold => 800,
            Weight::Heavy => 900,
            Weight::Ultraheavy => 1000,
            _ => 400,
        }
    }

    /// Style as a fontdb/cosmic-text enum, so downstream modules don't need
    /// to import pango to inspect it.
    pub fn ct_style(&self) -> cosmic_text::Style {
        match self.style {
            Style::Italic => cosmic_text::Style::Italic,
            Style::Oblique => cosmic_text::Style::Oblique,
            _ => cosmic_text::Style::Normal,
        }
    }
}

pub fn create_pango_layout_with_attrs(
    context: &Context,
    text: &str,
    attrs: Option<AttrList>,
    options: &FontAndLayoutOptions,
) -> Layout {
    PANGO_FONT_MAP.with(|font_map| {
        create_pango_layout_with_attrs_on_font_map(context, text, attrs, options, font_map)
    })
}

fn create_pango_layout_with_attrs_on_font_map(
    context: &Context,
    text: &str,
    attrs: Option<AttrList>,
    options: &FontAndLayoutOptions,
    font_map: &pango::FontMap,
) -> Layout {
    let FontAndLayoutOptions {
        letter_spacing,
        max_width,
        narrow,
        size,
        style,
        uppercase,
        weight,
    } = options;

    let pango_ctx = font_map.create_context();
    update_context(context, &pango_ctx);

    // Mapnik sizes assume 72dpi; Pango defaults to 96dpi. Set the layout context
    // resolution to 72dpi so we don't need the old 0.75 fudge factor.
    context_set_resolution(&pango_ctx, 72.0);

    let mut font_description = FontDescription::new();

    font_description.set_family(if *narrow {
        "MapRender Sans Narrow"
    } else {
        "MapRender Sans"
    });

    let mut fo = cairo::FontOptions::new().unwrap();
    fo.set_hint_style(cairo::HintStyle::None); // probably best
    fo.set_hint_metrics(cairo::HintMetrics::Off); // looks slightly nicer with Off
    // fo.set_antialias(cairo::Antialias::Subpixel); // no difference

    context_set_font_options(&pango_ctx, Some(&fo));
    pango_ctx.set_round_glyph_positions(false); // nicer with false

    font_description.set_weight(*weight);

    font_description.set_size((SCALE as f64 * size) as i32);

    font_description.set_style(*style);

    let layout = Layout::new(&pango_ctx);

    layout.set_font_description(Some(&font_description));

    // Line spacing should stay visually consistent across retina/non-retina scales.
    // Derive the current CTM scale from the context and normalize spacing by it.
    let (sx, sy) = context.user_to_device_distance(1.0, 1.0).unwrap();
    let scale = ((sx.abs() + sy.abs()) / 2.0).max(0.001);
    let line_spacing = 0.4 * (2.0 / scale);

    layout.set_wrap(WrapMode::Word);
    layout.set_alignment(Alignment::Center);
    layout.set_line_spacing(line_spacing as f32);
    layout.set_width((max_width * SCALE as f64) as i32);

    let text = if *uppercase {
        Cow::Owned(text.to_uppercase())
    } else {
        Cow::Borrowed(text)
    };

    layout.set_text(&text);

    let mut attr_list = attrs;

    if *letter_spacing > 0.0 {
        let list = attr_list.unwrap_or_default();

        list.insert(AttrInt::new_letter_spacing(
            (SCALE as f64 * *letter_spacing) as i32,
        ));

        attr_list = Some(list);
    }

    if let Some(ref list) = attr_list {
        layout.set_attributes(Some(list));
    }

    update_layout(context, &layout);

    layout
}

/// Detect tofu/notdef rendering for the given font configuration by laying a canary
/// glyph ("A") onto a recording surface and inspecting the resulting cairo path.
///
/// The .notdef glyph in sans-serif fonts is two nested rectangles: 0 curves, few line
/// segments, exactly 2 subpaths. The letter "A" in any standard font has either curves
/// or >> 8 line segments spread across 2 subpaths. If the probe path has 0 curves and
/// very few line segments, the font is rendering notdef — log it so we can correlate
/// with tile output.
#[derive(Debug, Clone, Copy)]
pub struct TofuProbeResult {
    pub move_tos: usize,
    pub line_tos: usize,
    pub curve_tos: usize,
    pub close_paths: usize,
}

impl TofuProbeResult {
    // Tofu signature: no curves at all, few line segments (one per rectangle side),
    // and at least one closed subpath. Any real sans-serif "A" has curves or many
    // more line segments; this threshold won't match it.
    pub fn looks_like_tofu(&self) -> bool {
        self.curve_tos == 0 && self.line_tos <= 10 && self.close_paths >= 1
    }
}

/// Lay the given canary text onto a recording surface with `options` and summarise
/// the resulting cairo path. Returns `None` if cairo refuses to create the surface
/// or copy the path.
pub fn probe_font_path(canary: &str, options: &FontAndLayoutOptions) -> Option<TofuProbeResult> {
    let surface = cairo::RecordingSurface::create(cairo::Content::Alpha, None).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    let layout = create_pango_layout_with_attrs(&cr, canary, None, options);
    cr.move_to(0.0, 0.0);
    pangocairo::functions::layout_path(&cr, &layout);
    let path = cr.copy_path().ok()?;

    let mut result = TofuProbeResult {
        move_tos: 0,
        line_tos: 0,
        curve_tos: 0,
        close_paths: 0,
    };

    for segment in path.iter() {
        match segment {
            cairo::PathSegment::MoveTo(_) => result.move_tos += 1,
            cairo::PathSegment::LineTo(_) => result.line_tos += 1,
            cairo::PathSegment::CurveTo(_, _, _) => result.curve_tos += 1,
            cairo::PathSegment::ClosePath => result.close_paths += 1,
        }
    }

    Some(result)
}

/// Detect tofu/notdef rendering for the given font configuration by laying the
/// canary glyph "A" on a recording surface. Returns `FreetypeError` when the
/// path signature matches two nested rectangles so the tile fails to render and
/// is not cached — rather caching a bad tile for everyone, fail this request and
/// let the next retry see fresh font state.
pub fn probe_font_tofu(options: &FontAndLayoutOptions) -> cairo::Result<()> {
    // Normally "A". Overridable at runtime for field-testing the detector —
    // e.g. MAPRENDER_TOFU_CANARY=□ forces the probe to look at a character
    // that renders as a rectangle outline, triggering the detector.
    let canary: &str = &CANARY;
    let Some(probe) = probe_font_path(canary, options) else {
        return Ok(());
    };

    if !probe.looks_like_tofu() {
        return Ok(());
    }

    let family = if options.narrow {
        "MapRender Sans Narrow"
    } else {
        "MapRender Sans"
    };

    eprintln!(
        "Tofu detected on canary {canary:?}: family={family} size={} weight={:?} style={:?} \
         path={{move={} line={} curve={} close={}}}",
        options.size,
        options.weight,
        options.style,
        probe.move_tos,
        probe.line_tos,
        probe.curve_tos,
        probe.close_paths,
    );

    Err(cairo::Error::FreetypeError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_flag_letter_a() {
        let opts = FontAndLayoutOptions::default();
        let probe = probe_font_path("A", &opts).expect("probe path");
        assert!(
            !probe.looks_like_tofu(),
            "letter A should not look like tofu, got {probe:?}",
        );
    }

    #[test]
    fn predicate_flags_tofu_signature() {
        // What a .notdef glyph (two nested rectangles) typically produces:
        // 2 MoveTos, 8 LineTos, 0 CurveTos, 2 ClosePaths.
        let tofu = TofuProbeResult {
            move_tos: 2,
            line_tos: 8,
            curve_tos: 0,
            close_paths: 2,
        };

        assert!(tofu.looks_like_tofu());
    }

    #[test]
    fn predicate_ignores_curved_glyph() {
        let real = TofuProbeResult {
            move_tos: 2,
            line_tos: 4,
            curve_tos: 6,
            close_paths: 2,
        };
        assert!(!real.looks_like_tofu());
    }
}
