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

thread_local! {
    static PANGO_FONT_MAP: pango::FontMap = FontMap::new();
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
