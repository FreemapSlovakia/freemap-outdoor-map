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
pub const DEFAULT_JPEG_QUALITY: f32 = 90.0;
pub const DEFAULT_WEBP_QUALITY: f32 = 80.0;

impl ImageFormat {
    /// Parse a raster format name, with an optional `=<quality>` for the lossy
    /// ones. `None` for a name this does not know; `Err` for a name it does and
    /// a quality it cannot accept.
    ///
    /// Lossy WebP only pays on an overlay dense enough that lossless has no
    /// sparsity to exploit; a sparse one encodes smaller lossless, and without
    /// the fringing a photo codec leaves around text.
    pub fn parse(token: &str) -> Option<Result<Self, String>> {
        let (name, quality) = token.split_once('=').map_or((token, None), |(n, q)| (n, Some(q)));

        let lossy = |default: f32| {
            quality.map_or(Ok(default), |q| match q.parse::<f32>() {
                Ok(q) if (0.0..=100.0).contains(&q) => Ok(q),
                Ok(q) => Err(format!("quality {q} is outside 0..=100")),
                Err(_) => Err(format!("quality '{q}' is not a number")),
            })
        };

        let lossless =
            || quality.map_or(Ok(()), |_| Err(format!("format '{name}' takes no quality")));

        Some(match name {
            "jpeg" | "jpg" => lossy(DEFAULT_JPEG_QUALITY).map(|q| Self::Jpeg(q as u8)),
            "webp-lossy" => lossy(DEFAULT_WEBP_QUALITY).map(|q| Self::Webp(WebpQuality::Lossy(q))),
            "png" => lossless().map(|()| Self::Png),
            "webp" => lossless().map(|()| Self::Webp(WebpQuality::Lossless)),
            _ => return None,
        })
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
