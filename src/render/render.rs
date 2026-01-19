use crate::render::{
    image_format::ImageFormat,
    layers::{self, hillshading_datasets::HillshadingDatasets},
    render_request::RenderRequest,
    svg_repo::SvgRepo,
    xyz::bbox_size_in_pixels,
};
use cairo::{
    Content, Context, Format, ImageSurface, PdfSurface, RecordingSurface, Rectangle, Surface,
    SvgSurface,
};
use geo::Geometry;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Error rendering layers: {0}")]
    LayersRenderError(#[from] layers::RenderError),

    #[error(transparent)]
    CairoError(#[from] cairo::Error),

    #[error("Error encoding image: {0}")]
    ImageEncodingError(Box<dyn std::error::Error + Send + Sync>),
}

pub fn render(
    request: &RenderRequest,
    client: &mut postgres::Client,
    svg_repo: &mut SvgRepo,
    hillshading_datasets: &mut Option<HillshadingDatasets>,
    mask_geometry: Option<&Geometry>,
) -> Result<Vec<Vec<u8>>, RenderError> {
    let _span = tracy_client::span!("render_tile");

    if request.scales.is_empty() {
        return Ok(Vec::new());
    }

    let size = bbox_size_in_pixels(request.bbox, request.zoom as f64);

    let scales = request.scales.clone();

    let mut render = |surface: &Surface, hillshade_scale: f64, render_scale: f64| {
        layers::render(
            surface,
            request,
            client,
            request.bbox,
            size,
            svg_repo,
            hillshading_datasets,
            hillshade_scale,
            mask_geometry,
            render_scale,
        )
    };

    match request.format {
        ImageFormat::Svg => {
            let primary_scale = scales.first().copied().unwrap_or(1.0);

            let surface = SvgSurface::for_stream(
                size.width as f64 * primary_scale,
                size.height as f64 * primary_scale,
                Vec::new(),
            )?;

            render(&surface, primary_scale.max(1.0), primary_scale)?;

            Ok(vec![
                *surface
                    .finish_output_stream()
                    .expect("finished output stream")
                    .downcast::<Vec<u8>>()
                    .expect("vector of bytes"),
            ])
        }
        ImageFormat::Pdf => {
            let primary_scale = scales.first().copied().unwrap_or(1.0);

            let surface = PdfSurface::for_stream(
                size.width as f64 * primary_scale,
                size.height as f64 * primary_scale,
                Vec::new(),
            )?;

            render(&surface, primary_scale.max(1.0), primary_scale)?;

            Ok(vec![
                *surface
                    .finish_output_stream()
                    .expect("finished output stream")
                    .downcast::<Vec<u8>>()
                    .expect("vector of bytes"),
            ])
        }
        ImageFormat::Png => {
            let max_scale = scales
                .iter()
                .copied()
                .fold(1.0_f64, |acc, scale| acc.max(scale));

            let recording_surface = RecordingSurface::create(
                Content::ColorAlpha,
                Some(Rectangle::new(
                    0.0,
                    0.0,
                    size.width as f64,
                    size.height as f64,
                )),
            )?;

            render(&recording_surface, max_scale.max(1.0), 1.0)?;

            let mut images = Vec::with_capacity(scales.len());

            for scale in scales {
                let mut buffer = Vec::new();

                let surface = ImageSurface::create(
                    Format::ARgb32,
                    (size.width as f64 * scale) as i32,
                    (size.height as f64 * scale) as i32,
                )?;

                if let Err(err) = paint_recording_surface(&recording_surface, &surface, scale) {
                    panic!("Error rendering {:?}@{}: {err}", request.bbox, request.zoom);
                }

                let _span = tracy_client::span!("render_tile::write_to_png");

                surface
                    .write_to_png(&mut buffer)
                    .map_err(|err| RenderError::ImageEncodingError(Box::new(err)))?;

                images.push(buffer);
            }

            Ok(images)
        }
        ImageFormat::Jpeg => {
            let max_scale = scales
                .iter()
                .copied()
                .fold(1.0_f64, |acc, scale| acc.max(scale));

            let recording_surface = RecordingSurface::create(
                Content::ColorAlpha,
                Some(Rectangle::new(
                    0.0,
                    0.0,
                    size.width as f64,
                    size.height as f64,
                )),
            )?;

            render(&recording_surface, max_scale.max(1.0), 1.0)?;

            let mut images = Vec::with_capacity(scales.len());

            for scale in scales {
                let mut surface = ImageSurface::create(
                    Format::Rgb24,
                    (size.width as f64 * scale) as i32,
                    (size.height as f64 * scale) as i32,
                )?;

                if let Err(err) = paint_recording_surface(&recording_surface, &surface, scale) {
                    panic!("Error rendering {:?}@{}: {err}", request.bbox, request.zoom);
                }

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

                JpegEncoder::new_with_quality(&mut buffer, 90)
                    .write_image(&rgb_data, width, height, ExtendedColorType::Rgb8)
                    .map_err(|err| RenderError::ImageEncodingError(Box::new(err)))?;

                images.push(buffer);
            }

            Ok(images)
        }
    }
}

fn paint_recording_surface(
    recording_surface: &RecordingSurface,
    target_surface: &Surface,
    scale: f64,
) -> cairo::Result<()> {
    let context = Context::new(target_surface)?;
    context.scale(scale, scale);
    context.set_source_surface(recording_surface, 0.0, 0.0)?;
    context.paint()?;
    Ok(())
}
