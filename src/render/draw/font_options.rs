pub use cosmic_text::{Style, Weight};

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
            weight: Weight::NORMAL,
        }
    }
}
