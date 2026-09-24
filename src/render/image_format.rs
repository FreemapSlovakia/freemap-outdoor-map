/// How a lossy WebP is encoded, or that it is not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WebpQuality {
    /// Exact. Smaller than lossy for sparse overlays - few colours and hard
    /// edges are what VP8L is for - and free of the fringing a photo codec
    /// leaves around text and thin lines.
    Lossless,
    /// Quality 0..=100. Worth it where the overlay covers enough of the tile
    /// that lossless has no sparsity to exploit.
    Lossy(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageFormat {
    Png,
    /// Quality 0..=100, as the encoder takes it — it clamps 0 up to 1.
    Jpeg(u8),
    Webp(WebpQuality),
    Pdf,
    Svg,
}

/// What each lossy format encodes at when nothing says otherwise. 90 is what
/// every tile has been encoded at since before the knob existed.
pub const DEFAULT_JPEG_QUALITY: u8 = 90;
pub const DEFAULT_WEBP_QUALITY: f32 = 80.0;

impl ImageFormat {
    /// Parse a raster format name and an optional quality. `None` for a name
    /// this does not know; `Err` for a name it does and a quality it cannot
    /// accept.
    ///
    /// Lossy WebP only pays on an overlay dense enough that lossless has no
    /// sparsity to exploit; a sparse one encodes smaller lossless, and without
    /// the fringing a photo codec leaves around text.
    pub fn from_parts(name: &str, quality: Option<f32>) -> Option<Result<Self, String>> {
        let lossy = |default: f32| match quality {
            None => Ok(default),
            Some(q) if (0.0..=100.0).contains(&q) => Ok(q),
            Some(q) => Err(format!("quality {q} is outside 0..=100")),
        };

        // Naming a quality for a format that has none is a mistake worth
        // hearing about from a config; an export says it is ignored, and passes
        // `None` for these rather than relying on this arm.
        let lossless =
            || quality.map_or(Ok(()), |_| Err(format!("format '{name}' takes no quality")));

        Some(match name {
            "jpeg" | "jpg" => {
                lossy(f32::from(DEFAULT_JPEG_QUALITY)).map(|q| Self::Jpeg(q as u8))
            }
            "webp-lossy" => lossy(DEFAULT_WEBP_QUALITY).map(|q| Self::Webp(WebpQuality::Lossy(q))),
            "png" => lossless().map(|()| Self::Png),
            "webp" => lossless().map(|()| Self::Webp(WebpQuality::Lossless)),
            _ => return None,
        })
    }

    /// Whether this name takes a quality at all.
    pub fn is_lossy(name: &str) -> bool {
        matches!(name, "jpeg" | "jpg" | "webp-lossy")
    }

    /// [`Self::from_parts`] over a `name[=quality]` token, as a tile variant
    /// spells it.
    pub fn parse(token: &str) -> Option<Result<Self, String>> {
        let (name, raw) = token.split_once('=').map_or((token, None), |(n, q)| (n, Some(q)));

        // A name this does not know is not a quality problem — say so before
        // looking at the other half, or a mistyped field reads as one.
        Self::from_parts(name, None)?.ok()?;

        let quality = match raw.map(str::parse::<f32>) {
            None => None,
            Some(Ok(q)) => Some(q),
            Some(Err(_)) => {
                return Some(Err(format!(
                    "quality '{}' is not a number",
                    raw.unwrap_or_default()
                )));
            }
        };

        Self::from_parts(name, quality)
    }

    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg(_) => "image/jpeg",
            Self::Webp(_) => "image/webp",
            Self::Pdf => "application/pdf",
            Self::Svg => "image/svg+xml",
        }
    }

    /// Whether the format keeps an alpha channel, and so can carry an overlay.
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Png | Self::Webp(_) | Self::Svg)
    }

    /// The extension cached tiles are stored under.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg(_) => "jpeg",
            Self::Webp(_) => "webp",
            Self::Pdf => "pdf",
            Self::Svg => "svg",
        }
    }
}
