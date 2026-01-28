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
use std::collections::HashSet;
use std::cell::RefCell;

thread_local! {
    static PANGO_FONT_MAP: RefCell<pango::FontMap> = RefCell::new(FontMap::new());
}

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

pub fn create_pango_layout_with_attrs(
    context: &Context,
    text: &str,
    attrs: Option<AttrList>,
    options: &FontAndLayoutOptions,
) -> Layout {
    PANGO_FONT_MAP.with(|font_map| {
        create_pango_layout_with_attrs_on_font_map(
            context,
            text,
            attrs,
            options,
            &font_map.borrow(),
        )
    })
}

pub fn create_pango_layout_with_attrs_fresh_map(
    context: &Context,
    text: &str,
    attrs: Option<AttrList>,
    options: &FontAndLayoutOptions,
) -> (Layout, pango::FontMap) {
    let font_map = FontMap::new();
    let layout =
        create_pango_layout_with_attrs_on_font_map(context, text, attrs, options, &font_map);
    (layout, font_map)
}

pub fn replace_thread_font_map(font_map: pango::FontMap) {
    PANGO_FONT_MAP.with(|current| {
        *current.borrow_mut() = font_map;
    });
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

    // layout.set_markup(r#"<span font_features="liga=1">fi</span>"#);

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

pub struct MissingGlyphsSummary {
    pub missing: usize,
    pub total: usize,
}

pub struct UniformGlyphsSummary {
    pub glyph: pango::Glyph,
    pub total: usize,
}

impl MissingGlyphsSummary {
    pub fn all_missing(&self) -> bool {
        self.total > 0 && self.missing == self.total
    }
}

pub fn log_missing_glyphs_layout(
    kind: &str,
    text: &str,
    layout: &Layout,
    point: Option<(f64, f64)>,
    missing: &MissingGlyphsSummary,
) {
    let desc = layout
        .font_description()
        .map(|d| d.to_str().to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let all_missing = if missing.all_missing() {
        "all"
    } else {
        "partial"
    };
    match point {
        Some((x, y)) => {
            eprintln!(
                "Missing glyphs ({all_missing}) in {kind} layout: text={text:?} point=({x:.2},{y:.2}) font={desc} missing={} total={}",
                missing.missing, missing.total
            );
        }
        None => {
            eprintln!(
                "Missing glyphs ({all_missing}) in {kind} layout: text={text:?} font={desc} missing={} total={}",
                missing.missing, missing.total
            );
        }
    }
}

pub fn create_layout_checked(
    context: &Context,
    kind: &str,
    text: &str,
    attrs: Option<AttrList>,
    options: &FontAndLayoutOptions,
    point: Option<(f64, f64)>,
) -> Result<Layout, cairo::Error> {
    let attrs_for_retry = attrs.as_ref().and_then(|list| list.copy());

    let mut layout = create_pango_layout_with_attrs(context, text, attrs, options);

    let mut missing = layout_missing_glyphs_summary_no_wrap(&layout);
    let uniform = layout_uniform_glyphs_summary(&layout);
    let suspicious_uniform = uniform.as_ref().map(|summary| summary.total >= 2).unwrap_or(false)
        && text_has_multiple_distinct_ascii_alnum(text);

    if missing.missing > 0 {
        log_missing_glyphs_layout(kind, text, &layout, point, &missing);
    }

    if suspicious_uniform {
        log_uniform_glyphs_layout(kind, text, &layout, point, uniform.as_ref().unwrap());
    }

    if missing.all_missing() || suspicious_uniform {
        let (fresh_layout, fresh_map) =
            create_pango_layout_with_attrs_fresh_map(context, text, attrs_for_retry, options);

        let fresh_missing = layout_missing_glyphs_summary_no_wrap(&fresh_layout);
        let fresh_uniform = layout_uniform_glyphs_summary(&fresh_layout);
        let fresh_suspicious_uniform =
            fresh_uniform.as_ref().map(|summary| summary.total >= 2).unwrap_or(false)
                && text_has_multiple_distinct_ascii_alnum(text);

        if !fresh_missing.all_missing() && !fresh_suspicious_uniform {
            replace_thread_font_map(fresh_map);
            layout = fresh_layout;
            missing = fresh_missing;
            eprintln!("Recovered missing glyphs with fresh font map: text={text:?}");
        }
    }

    if missing.all_missing() {
        eprintln!("Recovery (glyphs) did not help!");

        return Err(cairo::Error::InvalidString);
    }

    Ok(layout)
}

fn layout_missing_glyphs_summary_no_wrap(layout: &Layout) -> MissingGlyphsSummary {
    let width = layout.width();
    let wrap = layout.wrap();

    layout.set_width(-1);
    layout.set_wrap(WrapMode::Word);

    let missing = layout_missing_glyphs_summary(layout);

    layout.set_width(width);
    layout.set_wrap(wrap);

    missing
}

fn layout_missing_glyphs_summary(layout: &Layout) -> MissingGlyphsSummary {
    let mut iter = layout.iter();
    let mut missing_count = 0_usize;
    let mut total_count = 0_usize;

    loop {
        if let Some(run) = iter.run_readonly() {
            let glyphs = run.glyph_string();

            for info in glyphs.glyph_info() {
                let glyph = info.glyph();

                total_count += 1;

                if glyph == pango::GLYPH_EMPTY
                    || glyph == pango::GLYPH_INVALID_INPUT
                    || (glyph & pango::GLYPH_UNKNOWN_FLAG) != 0
                {
                    missing_count += 1;
                }
            }
        }

        if !iter.next_run() {
            break;
        }
    }

    MissingGlyphsSummary {
        missing: missing_count,
        total: total_count,
    }
}

fn layout_uniform_glyphs_summary(layout: &Layout) -> Option<UniformGlyphsSummary> {
    let mut iter = layout.iter();
    let mut total_count = 0_usize;
    let mut distinct = HashSet::new();

    loop {
        if let Some(run) = iter.run_readonly() {
            let glyphs = run.glyph_string();

            for info in glyphs.glyph_info() {
                let glyph = info.glyph();
                total_count += 1;
                distinct.insert(glyph);
            }
        }

        if !iter.next_run() {
            break;
        }
    }

    if total_count >= 2 && distinct.len() == 1 {
        let glyph = *distinct.iter().next().unwrap();
        Some(UniformGlyphsSummary {
            glyph,
            total: total_count,
        })
    } else {
        None
    }
}

fn text_has_multiple_distinct_ascii_alnum(text: &str) -> bool {
    let mut distinct = HashSet::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            distinct.insert(ch.to_ascii_lowercase());
            if distinct.len() >= 2 {
                return true;
            }
        }
    }

    false
}

fn log_uniform_glyphs_layout(
    kind: &str,
    text: &str,
    layout: &Layout,
    point: Option<(f64, f64)>,
    uniform: &UniformGlyphsSummary,
) {
    let desc = layout
        .font_description()
        .map(|d| d.to_str().to_string())
        .unwrap_or_else(|| "<none>".to_string());
    match point {
        Some((x, y)) => {
            eprintln!(
                "Suspicious glyphs (uniform) in {kind} layout: text={text:?} point=({x:.2},{y:.2}) font={desc} glyph={} total={}",
                uniform.glyph, uniform.total
            );
        }
        None => {
            eprintln!(
                "Suspicious glyphs (uniform) in {kind} layout: text={text:?} font={desc} glyph={} total={}",
                uniform.glyph, uniform.total
            );
        }
    }
}
