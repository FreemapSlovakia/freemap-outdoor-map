use crate::render::{
    PlaceTypeOverrides,
    attribution::Attribution,
    image_format::{ImageFormat, WebpQuality},
    layers,
    render_request::RenderRequest,
    svg_repo::SvgRepo,
    xyz::bbox_size_in_pixels,
};
use cairo::{Format, ImageSurface, PdfSurface, Surface, SvgSurface, SvgUnit};
use deadpool_postgres::Pool;
use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::{ExtendedColorType, ImageEncoder};
use std::sync::Arc;
use tokio::runtime::Handle;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Error rendering layers: {0}")]
    Layers(#[from] layers::RenderError),

    #[error(transparent)]
    Cairo(#[from] cairo::Error),

    #[error("Error encoding image: {0}")]
    ImageEncoding(Box<dyn std::error::Error + Send + Sync>),
}

/// A finished render: the encoded image, and the codes of the datasets that
/// contributed pixels to it.
pub struct RenderOutput {
    pub bytes: Vec<u8>,
    pub attribution: Attribution,
    /// Nothing was painted — every pixel fully transparent. An overlay produces
    /// these by the thousand, and the cache stores them as a mark rather than a
    /// file. Always false for a format without alpha, which paints its ground.
    pub blank: bool,
}

/// A map pixel is 1/96 in, as on the web.
const PX_PER_IN: f64 = 96.0;

const PT_PER_PX: f64 = 72.0 / PX_PER_IN;

/// Pixel density to write into a raster image, at least 1 so the header stays
/// valid.
fn print_dpi(request: &RenderRequest) -> Option<f64> {
    request
        .print_density
        .then(|| (PX_PER_IN * request.scale).max(1.0))
}

/// The pipeline scales its context by `scale` so raster content (shading,
/// raster tiles) is sampled at `PX_PER_IN × scale` DPI. On a vector page that would
/// also grow the page, so the device scale takes it back out, leaving the page
/// at its true print size.
fn set_vector_device_scale(surface: &Surface, scale: f64) {
    surface.set_device_scale(PT_PER_PX / scale, PT_PER_PX / scale);
}

pub fn render(
    request: &RenderRequest,
    shading: layers::Shading<'_>,
    place_type_overrides: Option<Arc<PlaceTypeOverrides>>,
    pool: Pool,
    handle: Handle,
    svg_repo: &mut SvgRepo,
) -> Result<RenderOutput, RenderError> {
    let _span = tracy_client::span!("render_tile");

    let size = bbox_size_in_pixels(request.bbox, request.zoom as f64);

    let render = |surface: &Surface| {
        layers::render(
            surface,
            request,
            shading,
            place_type_overrides,
            pool,
            handle,
            size,
            svg_repo,
        )
    };

    match request.format {
        ImageFormat::Svg => {
            let mut surface = SvgSurface::for_stream(
                size.width as f64 * PT_PER_PX,
                size.height as f64 * PT_PER_PX,
                Vec::new(),
            )?;

            // Cairo's default writes the size unitless, which reads as CSS px.
            surface.set_document_unit(SvgUnit::Pt);

            set_vector_device_scale(&surface, request.scale);

            let attribution = render(&surface)?;

            Ok(RenderOutput {
                blank: false,
                bytes: *surface
                    .finish_output_stream()
                    .expect("finished output stream")
                    .downcast::<Vec<u8>>()
                    .expect("vector of bytes"),
                attribution,
            })
        }
        ImageFormat::Pdf => {
            let surface = PdfSurface::for_stream(
                size.width as f64 * PT_PER_PX,
                size.height as f64 * PT_PER_PX,
                Vec::new(),
            )?;

            set_vector_device_scale(&surface, request.scale);

            // Cairo rasterizes what PDF cannot express; this gives such regions
            // 300/72 × `scale` px per map px.
            let fallback_dpi = 300.0 / 72.0 * PX_PER_IN * request.scale;
            surface.set_fallback_resolution(fallback_dpi, fallback_dpi);

            let attribution = render(&surface)?;

            Ok(RenderOutput {
                blank: false,
                bytes: *surface
                    .finish_output_stream()
                    .expect("finished output stream")
                    .downcast::<Vec<u8>>()
                    .expect("vector of bytes"),
                attribution,
            })
        }
        ImageFormat::Webp(quality) => {
            let scale = request.scale;

            let mut surface = ImageSurface::create(
                Format::ARgb32,
                (size.width as f64 * scale) as i32,
                (size.height as f64 * scale) as i32,
            )?;

            let attribution = render(&surface)?;

            let _span = tracy_client::span!("render_tile::encode_webp");

            let (rgba, width, height) = argb32_to_rgba(&mut surface);

            let encoder = webp::Encoder::from_rgba(&rgba, width, height);

            // `encode`/`encode_lossless` unwrap internally, and a render worker has
            // no unwind guard — one oversized surface would kill it for the life of
            // the process. libwebp caps a side at 16383 px, which an export's
            // `scale` can exceed.
            let (lossless, q) = match quality {
                WebpQuality::Lossless => (true, 75.0),
                WebpQuality::Lossy(q) => (false, q),
            };

            let encoded = encoder
                .encode_simple(lossless, q)
                .map_err(|err| RenderError::ImageEncoding(format!("{err:?}").into()))?;

            let bytes = encoded.to_vec();

            Ok(RenderOutput {
                bytes: match print_dpi(request) {
                    Some(dpi) => with_webp_exif_dpi(bytes, dpi),
                    None => bytes,
                },
                attribution,
                blank: surface_is_blank(&mut surface),
            })
        }
        ImageFormat::Png => {
            let scale = request.scale;

            let mut buffer = Vec::new();

            let mut surface = ImageSurface::create(
                Format::ARgb32,
                (size.width as f64 * scale) as i32,
                (size.height as f64 * scale) as i32,
            )?;

            let attribution = render(&surface)?;

            let _span = tracy_client::span!("render_tile::write_to_png");

            surface
                .write_to_png(&mut buffer)
                .map_err(|err| RenderError::ImageEncoding(Box::new(err)))?;

            Ok(RenderOutput {
                bytes: match print_dpi(request) {
                    Some(dpi) => with_png_phys(buffer, dpi),
                    None => buffer,
                },
                attribution,
                blank: surface_is_blank(&mut surface),
            })
        }
        ImageFormat::Jpeg(quality) => {
            let scale = request.scale;

            let mut surface = ImageSurface::create(
                Format::Rgb24,
                (size.width as f64 * scale) as i32,
                (size.height as f64 * scale) as i32,
            )?;

            let attribution = render(&surface)?;

            let width = surface.width() as u32;
            let height = surface.height() as u32;
            let stride = surface.stride() as usize;
            let data = surface.data().expect("surface data");

            let mut rgb_data = Vec::with_capacity((width * height * 3) as usize);

            for y in 0..height as usize {
                let row_start = y * stride;
                let row_end = row_start + width as usize * 4;
                let row = &data[row_start..row_end];

                for chunk in row.chunks(4) {
                    let b = chunk[0];
                    let g = chunk[1];
                    let r = chunk[2];

                    rgb_data.extend_from_slice(&[r, g, b]);
                }
            }

            let mut buffer = Vec::new();

            let mut encoder = JpegEncoder::new_with_quality(&mut buffer, quality);

            if let Some(dpi) = print_dpi(request) {
                encoder.set_pixel_density(PixelDensity::dpi(dpi.round() as u16));
            }

            encoder
                .write_image(&rgb_data, width, height, ExtendedColorType::Rgb8)
                .map_err(|err| RenderError::ImageEncoding(Box::new(err)))?;

            Ok(RenderOutput {
                bytes: buffer,
                attribution,
                blank: false,
            })
        }
    }
}

/// Cairo's `ARgb32` is premultiplied BGRA in native byte order; WebP wants
/// straight RGBA. Undoing the premultiply is what keeps a half-transparent
/// label its own colour instead of a darkened one.
fn argb32_to_rgba(surface: &mut ImageSurface) -> (Vec<u8>, u32, u32) {
    let width = surface.width() as u32;
    let height = surface.height() as u32;
    let stride = surface.stride() as usize;
    let data = surface.data().expect("surface data");

    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height as usize {
        let row_start = y * stride;
        let row = &data[row_start..row_start + width as usize * 4];

        for chunk in row.chunks_exact(4) {
            let (b, g, r, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);

            // Overlays are drawn opaque, so all but the antialiased edges take
            // one of the two cheap arms; the divide is for the fringe.
            if a == 0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else if a == 255 {
                rgba.extend_from_slice(&[r, g, b, 255]);
            } else {
                let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;

                rgba.extend_from_slice(&[un(r), un(g), un(b), a]);
            }
        }
    }

    (rgba, width, height)
}

/// Whether the surface has any painted pixel. Stops at the first one, so a tile
/// with content pays almost nothing and only a truly blank one is walked whole.
fn surface_is_blank(surface: &mut ImageSurface) -> bool {
    let width = surface.width() as usize;
    let height = surface.height() as usize;
    let stride = surface.stride() as usize;
    let data = surface.data().expect("surface data");

    // Premultiplied `ARgb32`, so a zero alpha is a zero pixel; the byte at index
    // 3 of each chunk is the alpha on every little-endian target cairo builds for.
    (0..height).all(|y| {
        let row = &data[y * stride..y * stride + width * 4];

        row.chunks_exact(4).all(|px| px[3] == 0)
    })
}

/// Cairo writes no pHYs, so print size would fall to the viewer's guess. It is
/// spliced right after IHDR, which the PNG format fixes at 13 bytes after the
/// 8-byte signature.
fn with_png_phys(mut png: Vec<u8>, dpi: f64) -> Vec<u8> {
    const IHDR_END: usize = 8 + 4 + 4 + 13 + 4;

    let ppm = (dpi / 0.0254).round() as u32;

    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9u32.to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&ppm.to_be_bytes());
    chunk.extend_from_slice(&ppm.to_be_bytes());
    chunk.push(1); // unit: metre
    chunk.extend_from_slice(&crc32fast::hash(&chunk[4..]).to_be_bytes());

    png.splice(IHDR_END..IHDR_END, chunk);

    png
}

/// WebP has no density field of its own; EXIF resolution is what GIMP and
/// `ImageMagick` read. An EXIF chunk needs the extended (`VP8X`) layout, which a
/// simple-format file is converted to.
fn with_webp_exif_dpi(mut webp: Vec<u8>, dpi: f64) -> Vec<u8> {
    const VP8X_FLAG_EXIF: u8 = 0x08;
    const VP8X_FLAG_ALPHA: u8 = 0x10;

    let first = &webp[12..16];

    if first != b"VP8X" {
        let data = &webp[20..];

        let (width, height, alpha) = if first == b"VP8L" {
            let bits = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);

            (
                (bits & 0x3fff) + 1,
                ((bits >> 14) & 0x3fff) + 1,
                bits >> 28 & 1 == 1,
            )
        } else {
            (
                u32::from(u16::from_le_bytes([data[6], data[7]]) & 0x3fff),
                u32::from(u16::from_le_bytes([data[8], data[9]]) & 0x3fff),
                false,
            )
        };

        let mut vp8x = Vec::with_capacity(18);
        vp8x.extend_from_slice(b"VP8X");
        vp8x.extend_from_slice(&10u32.to_le_bytes());
        vp8x.push(if alpha { VP8X_FLAG_ALPHA } else { 0 });
        vp8x.extend_from_slice(&[0; 3]);
        vp8x.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        vp8x.extend_from_slice(&(height - 1).to_le_bytes()[..3]);

        webp.splice(12..12, vp8x);
    }

    webp[20] |= VP8X_FLAG_EXIF;

    // Little-endian TIFF: one IFD with XResolution, YResolution and
    // ResolutionUnit (inches), then the two rationals it points at.
    let density = [(dpi * 100.0).round() as u32, 100];

    let mut exif = Vec::with_capacity(66);
    exif.extend_from_slice(b"II*\0");
    exif.extend_from_slice(&8u32.to_le_bytes());
    exif.extend_from_slice(&3u16.to_le_bytes());

    for (tag, typ, value) in [(0x011a_u16, 5_u16, 50_u32), (0x011b, 5, 58), (0x0128, 3, 2)] {
        exif.extend_from_slice(&tag.to_le_bytes());
        exif.extend_from_slice(&typ.to_le_bytes());
        exif.extend_from_slice(&1u32.to_le_bytes());
        exif.extend_from_slice(&value.to_le_bytes());
    }

    exif.extend_from_slice(&0u32.to_le_bytes());

    for _ in 0..2 {
        exif.extend_from_slice(&density[0].to_le_bytes());
        exif.extend_from_slice(&density[1].to_le_bytes());
    }

    webp.extend_from_slice(b"EXIF");
    webp.extend_from_slice(&(exif.len() as u32).to_le_bytes());
    webp.extend_from_slice(&exif);

    let riff_size = (webp.len() - 8) as u32;
    webp[4..8].copy_from_slice(&riff_size.to_le_bytes());

    webp
}
