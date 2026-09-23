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

#[derive(Debug, Clone, Copy)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp(WebpQuality),
    Pdf,
    Svg,
}

impl ImageFormat {
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
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
            Self::Jpeg => "jpeg",
            Self::Webp(_) => "webp",
            Self::Pdf => "pdf",
            Self::Svg => "svg",
        }
    }
}
