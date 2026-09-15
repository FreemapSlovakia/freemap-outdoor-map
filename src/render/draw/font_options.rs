use cosmic_text::{Attrs, Family};
pub use cosmic_text::{Style, Weight};
use std::borrow::Cow;

/// Font attributes of a label.
pub fn label_attrs(flo: &FontAndLayoutOptions) -> Attrs<'static> {
    Attrs::new()
        .family(Family::Name(if flo.narrow {
            "PT Sans Narrow"
        } else {
            "PT Sans"
        }))
        .weight(flo.weight)
        .style(flo.style)
        .letter_spacing((flo.letter_spacing / flo.size.max(0.0001)) as f32)
}

/// Label text as drawn.
pub fn label_text<'a>(text: &'a str, flo: &FontAndLayoutOptions) -> Cow<'a, str> {
    if flo.uppercase {
        Cow::Owned(uppercase_label(text))
    } else {
        Cow::Borrowed(text)
    }
}

/// Identifies a label's shaping: its text and the options [`label_attrs`], [`label_text`] and
/// the metrics read. `max_width` is left out, so only unwrapped labels may share keys.
#[derive(PartialEq, Eq, Hash)]
pub struct ShapeKey {
    text: String,
    size: u64,
    letter_spacing: u64,
    narrow: bool,
    uppercase: bool,
    style: Style,
    weight: Weight,
}

impl ShapeKey {
    pub fn new(text: &str, flo: &FontAndLayoutOptions) -> Self {
        Self {
            text: text.to_owned(),
            size: flo.size.to_bits(),
            letter_spacing: flo.letter_spacing.to_bits(),
            narrow: flo.narrow,
            uppercase: flo.uppercase,
            style: flo.style,
            weight: flo.weight,
        }
    }
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

/// Uppercase `text` for display, leaving Georgian untouched.
///
/// Georgian is a unicameral script: `str::to_uppercase` maps Mkhedruli
/// (U+10D0–U+10FF) to Mtavruli (Georgian Extended, U+1C90–U+1CBF), a titling
/// style our bundled fonts don't cover — so the result renders as tofu. Since
/// uppercasing Georgian is typographically wrong anyway, keep those letters as
/// is and uppercase everything else normally.
pub fn uppercase_label(text: &str) -> String {
    if text.chars().any(|c| ('\u{10D0}'..='\u{10FF}').contains(&c)) {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if ('\u{10D0}'..='\u{10FF}').contains(&c) {
                out.push(c);
            } else {
                out.extend(c.to_uppercase());
            }
        }
        out
    } else {
        text.to_uppercase()
    }
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
            weight: Weight::NORMAL,
        }
    }
}
