use crate::render::{
    image_format::ImageFormat,
    layers::{self, HillshadingDatasets},
    render_request::RenderRequest,
    svg_repo::SvgRepo,
    xyz::bbox_size_in_pixels,
};
use cairo::{Format, ImageSurface, PdfSurface, Surface, SvgSurface};
use geo::Geometry;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Error rendering layers: {0}")]
    Layers(#[from] layers::RenderError),

    #[error(transparent)]
    Cairo(#[from] cairo::Error),

    #[error("Error encoding image: {0}")]
    ImageEncoding(Box<dyn std::error::Error + Send + Sync>),
}

pub fn render(
    request: &RenderRequest,
    client: &mut postgres::Client,
    svg_repo: &mut SvgRepo,
    hillshading_datasets: &mut Option<HillshadingDatasets>,
    mask_geometry: Option<&Geometry>,
) -> Result<Vec<u8>, RenderError> {
    let _span = tracy_client::span!("render_tile");

    let size = bbox_size_in_pixels(request.bbox, request.zoom as f64);

    let mut render = |surface: &Surface| {
        layers::render(
            surface,
            request,
            client,
            request.bbox,
            size,
            svg_repo,
            hillshading_datasets,
            mask_geometry,
            request.scale,
        )
    };

    match request.format {
        ImageFormat::Svg => {
            let scale = request.scale;

            let surface = SvgSurface::for_stream(
                size.width as f64 * scale,
                size.height as f64 * scale,
                Vec::new(),
            )?;

            render(&surface)?;

            Ok(*surface
                .finish_output_stream()
                .expect("finished output stream")
                .downcast::<Vec<u8>>()
                .expect("vector of bytes"))
        }
        ImageFormat::Pdf => {
            let scale = request.scale;

            let surface = PdfSurface::for_stream(
                size.width as f64 * scale,
                size.height as f64 * scale,
                Vec::new(),
            )?;

            render(&surface)?;

            Ok(*surface
                .finish_output_stream()
                .expect("finished output stream")
                .downcast::<Vec<u8>>()
                .expect("vector of bytes"))
        }
        ImageFormat::Png => {
            let scale = request.scale;

            let mut buffer = Vec::new();

            let surface = ImageSurface::create(
                Format::ARgb32,
                (size.width as f64 * scale) as i32,
                (size.height as f64 * scale) as i32,
            )?;

            render(&surface)?;

            let _span = tracy_client::span!("render_tile::write_to_png");

            surface
                .write_to_png(&mut buffer)
                .map_err(|err| RenderError::ImageEncoding(Box::new(err)))?;

            Ok(buffer)
        }
        ImageFormat::Jpeg => {
            let scale = request.scale;

            let mut surface = ImageSurface::create(
                Format::Rgb24,
                (size.width as f64 * scale) as i32,
                (size.height as f64 * scale) as i32,
            )?;

            render(&surface)?;

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
                .map_err(|err| RenderError::ImageEncoding(Box::new(err)))?;

            Ok(buffer)
        }
    }
}
