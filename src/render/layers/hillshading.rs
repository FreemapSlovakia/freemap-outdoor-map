use crate::render::{
    ctx::Ctx,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::hillshading_datasets::HillshadingDatasets,
};
use cairo::{Context, Format, ImageSurface};
use gdal::Dataset;

pub enum Mode {
    Mask,
    Shading,
}

/// In-place morphological erosion of a 0/255 mask by a rectangular structuring
/// element (separable min-filter, independent radius per axis). Shrinks the
/// valid region so output pixels whose Lanczos footprint could reach a masked
/// source pixel get dropped. Operates on the tile-sized resampled buffer.
fn erode(mask: &mut [u8], width: usize, height: usize, radius_x: usize, radius_y: usize) {
    if width == 0 || height == 0 || (radius_x == 0 && radius_y == 0) {
        return;
    }

    let mut tmp = mask.to_vec();

    // horizontal pass: mask -> tmp
    for y in 0..height {
        let row = y * width;
        for x in 0..width {
            let lo = x.saturating_sub(radius_x);
            let hi = (x + radius_x).min(width - 1);
            tmp[row + x] = mask[row + lo..=row + hi]
                .iter()
                .copied()
                .min()
                .expect("window is non-empty: lo <= x <= hi");
        }
    }

    // vertical pass: tmp -> mask
    for x in 0..width {
        for y in 0..height {
            let lo = y.saturating_sub(radius_y);
            let hi = (y + radius_y).min(height - 1);
            let mut m = 255u8;
            for k in lo..=hi {
                m = m.min(tmp[k * width + x]);
            }
            mask[y * width + x] = m;
        }
    }
}

fn read_rgba_from_gdal(
    dataset: &Dataset,
    ctx: &Ctx,
    mode: Mode,
) -> Result<Option<ImageSurface>, LayerRenderError> {
    let bbox = ctx.bbox;
    let size = ctx.size;

    let min = bbox.min();
    let max = bbox.max();

    let [gt_x_off, gt_x_width, _, gt_y_off, _, gt_y_width] = dataset.geo_transform()?;

    // Convert geographic coordinates (min_x, min_y, max_x, max_y) to pixel coordinates
    let pixel_min_x_f = (min.x - gt_x_off) / gt_x_width;
    let pixel_max_x_f = (max.x - gt_x_off) / gt_x_width;

    let pixel_min_x = pixel_min_x_f.floor() as isize;
    let pixel_max_x = pixel_max_x_f.ceil() as isize;

    let (pixel_min_y_f, pixel_max_y_f) = {
        let pixel_y0 = (min.y - gt_y_off) / gt_y_width;
        let pixel_y1 = (max.y - gt_y_off) / gt_y_width;

        (pixel_y0.min(pixel_y1), pixel_y0.max(pixel_y1))
    };

    let pixel_min_y = pixel_min_y_f.floor() as isize;
    let pixel_max_y = pixel_max_y_f.ceil() as isize;

    let window_width_px = (pixel_max_x - pixel_min_x) as usize;
    let window_height_px = (pixel_max_y - pixel_min_y) as usize;

    let scaled_width_px = (size.width as f64 * ctx.scale) as usize;
    let scaled_height_px = (size.height as f64 * ctx.scale) as usize;

    let scale_x = scaled_width_px as f64 / (pixel_max_x_f - pixel_min_x_f).abs().max(1e-6);
    let scale_y = scaled_height_px as f64 / (pixel_max_y_f - pixel_min_y_f).abs().max(1e-6);

    let buffered_w = (scale_x * window_width_px as f64).ceil().max(1.0) as usize;
    let buffered_h = (scale_y * window_height_px as f64).ceil().max(1.0) as usize;

    let mut rgba_data = vec![0u8; buffered_w * buffered_h * 4];

    let (raster_width, raster_height) = dataset.raster_size();

    // Adjust the window to fit within the raster bounds
    let clamped_window_x = pixel_min_x.max(0).min(raster_width as isize);
    let clamped_window_y = pixel_min_y.max(0).min(raster_height as isize);

    let clamped_source_width = ((pixel_min_x + window_width_px as isize).min(raster_width as isize)
        - clamped_window_x)
        .max(0) as usize;

    let clamped_source_height =
        ((pixel_min_y + window_height_px as isize).min(raster_height as isize) - clamped_window_y)
            .max(0) as usize;

    if clamped_source_width == 0 || clamped_source_height == 0 {
        return Ok(None);
    }

    let resampled_width = (buffered_w as f64
        * (clamped_source_width as f64 / window_width_px as f64))
        .ceil() as usize;

    let resampled_height = (buffered_h as f64
        * (clamped_source_height as f64 / window_height_px as f64))
        .ceil() as usize;

    let offset_x = (((clamped_window_x - pixel_min_x) as f64 / window_width_px as f64)
        * buffered_w as f64)
        .floor()
        .max(0.0) as usize;

    let offset_y = (((clamped_window_y - pixel_min_y) as f64 / window_height_px as f64)
        * buffered_h as f64)
        .floor()
        .max(0.0) as usize;

    let copy_width = resampled_width.min(buffered_w.saturating_sub(offset_x));
    let copy_height = resampled_height.min(buffered_h.saturating_sub(offset_y));

    let mut band_buffer = vec![0u8; resampled_height * resampled_width];

    assert!(dataset.raster_count() == 4, "unsupported band count");

    if matches!(mode, Mode::Shading) {
        for band_index in 0..3 {
            let band = dataset.rasterband(band_index + 1)?;

            if clamped_source_width > 0
                && clamped_source_height > 0
                && resampled_width > 0
                && resampled_height > 0
            {
                band.read_into_slice::<u8>(
                    (clamped_window_x, clamped_window_y),
                    (clamped_source_width, clamped_source_height),
                    (resampled_width, resampled_height), // Resampled size
                    &mut band_buffer,
                    Some(gdal::raster::ResampleAlg::Lanczos),
                )?;
            }

            for y in 0..copy_height {
                for x in 0..copy_width {
                    let data_index = y * resampled_width + x;
                    let rgba_index = ((y + offset_y) * buffered_w + (x + offset_x)) * 4;
                    rgba_data[rgba_index + band_index] = band_buffer[data_index];
                }
            }
        }
    }

    let alpha_band = dataset.rasterband(4)?;

    let alpha_no_data = alpha_band.no_data_value().map(|nd| nd as u8);

    let mask_band = alpha_band
        .mask_flags()
        .ok()
        .filter(|f| {
            // println!(
            //     "MASK: {country} all_valid={} alpha={} nodata={} per_dataset={}",
            //     f.is_all_valid(),
            //     f.is_alpha(),
            //     f.is_nodata(),
            //     f.is_per_dataset()
            // );

            f.is_per_dataset()
        })
        .and_then(|_| alpha_band.open_mask_band().ok());

    // Read the per-dataset mask at the resampled (tile) resolution using
    // NearestNeighbour (no Lanczos ringing in the mask itself), then erode it so
    // any output pixel whose Lanczos footprint could reach a masked source pixel
    // gets dropped. The Lanczos support is 3 source pixels; expressed in output
    // pixels that is 3 * max(upscale, 1), so the radius only grows when
    // overzooming (where the buffer is small anyway).
    let eroded_mask = if let (Some(mask_band), true) = (
        mask_band.as_ref(),
        resampled_width > 0 && resampled_height > 0,
    ) {
        let mut buf = vec![0u8; resampled_width * resampled_height];

        mask_band.read_into_slice::<u8>(
            (clamped_window_x, clamped_window_y),
            (clamped_source_width, clamped_source_height),
            (resampled_width, resampled_height),
            &mut buf,
            Some(gdal::raster::ResampleAlg::NearestNeighbour),
        )?;

        let ratio_x = resampled_width as f64 / clamped_source_width as f64;
        let ratio_y = resampled_height as f64 / clamped_source_height as f64;
        let radius_x = (3.0 * ratio_x.max(1.0)).ceil() as usize;
        let radius_y = (3.0 * ratio_y.max(1.0)).ceil() as usize;

        erode(
            &mut buf,
            resampled_width,
            resampled_height,
            radius_x,
            radius_y,
        );

        Some(buf)
    } else {
        None
    };

    if clamped_source_width > 0
        && clamped_source_height > 0
        && resampled_width > 0
        && resampled_height > 0
    {
        alpha_band.read_into_slice::<u8>(
            (clamped_window_x, clamped_window_y),
            (clamped_source_width, clamped_source_height),
            (resampled_width, resampled_height), // Resampled size
            &mut band_buffer,
            Some(gdal::raster::ResampleAlg::Lanczos),
        )?;
    }

    let mut has_data = false;

    for y in 0..copy_height {
        for x in 0..copy_width {
            let (alpha, mask_alpha) = {
                let data_index = y * resampled_width + x;

                let value = band_buffer[data_index];

                if alpha_no_data.is_some_and(|nd| nd == value)
                    || eroded_mask.as_ref().is_some_and(|m| m[data_index] == 0)
                {
                    (0, 0)
                } else {
                    has_data = true;

                    (value, 255)
                }
            };

            let rgba_index = ((y + offset_y) * buffered_w + (x + offset_x)) * 4;

            match mode {
                Mode::Shading => {
                    rgba_data[rgba_index + 3] = alpha;
                }
                Mode::Mask => {
                    rgba_data[rgba_index] = 255;
                    rgba_data[rgba_index + 1] = 255;
                    rgba_data[rgba_index + 2] = 255;
                    rgba_data[rgba_index + 3] = mask_alpha;
                }
            }
        }
    }

    let (crop_x, crop_y) = {
        let frac_x = pixel_min_x_f - pixel_min_x as f64;
        let frac_y = pixel_min_y_f - pixel_min_y as f64;

        let crop_x_base = offset_x + (frac_x * scale_x).round().max(0.0) as usize;
        let crop_y_base = offset_y + (frac_y * scale_y).round().max(0.0) as usize;

        // If rounding pushed the origin too far, clamp so we still copy a full tile when possible.
        let crop_x = crop_x_base.min(buffered_w.saturating_sub(scaled_width_px));
        let crop_y = crop_y_base.min(buffered_h.saturating_sub(scaled_height_px));

        (crop_x, crop_y)
    };

    let crop_w = scaled_width_px.min(buffered_w.saturating_sub(crop_x));
    let crop_h = scaled_height_px.min(buffered_h.saturating_sub(crop_y));

    let mut final_rgba_data = vec![0u8; scaled_width_px * scaled_height_px * 4];

    if crop_w > 0 && crop_h > 0 && crop_x < buffered_w && crop_y < buffered_h {
        for y in 0..crop_h {
            let src_offset = ((y + crop_y) * buffered_w + crop_x) * 4;
            let dst_offset = y * scaled_width_px * 4;

            // Guard against any edge rounding that would push past the buffer.
            let max_copy = ((buffered_w - crop_x) * 4).min(crop_w * 4);
            let src_end = (src_offset + max_copy).min(rgba_data.len());
            let dst_end = dst_offset + (src_end - src_offset);

            if src_end > src_offset && dst_end > dst_offset {
                final_rgba_data[dst_offset..dst_end]
                    .copy_from_slice(&rgba_data[src_offset..src_end]);
            }
        }
    }

    for i in (0..final_rgba_data.len()).step_by(4) {
        let alpha = final_rgba_data[i + 3] as f32 / 255.0;

        let r = (final_rgba_data[i] as f32 * alpha) as u8;
        let g = (final_rgba_data[i + 1] as f32 * alpha) as u8;
        let b = (final_rgba_data[i + 2] as f32 * alpha) as u8;

        final_rgba_data[i] = b;
        final_rgba_data[i + 1] = g;
        final_rgba_data[i + 2] = r;
    }

    if !has_data {
        return Ok(None);
    }

    let surface = ImageSurface::create_for_data(
        final_rgba_data,
        Format::ARgb32,
        (size.width as f64 * ctx.scale) as i32,
        (size.height as f64 * ctx.scale) as i32,
        (size.width as f64 * ctx.scale) as i32 * 4,
    )?;

    Ok(Some(surface))
}

pub fn load_surface(
    ctx: &Ctx,
    country: &str,
    shading_data: &HillshadingDatasets,
    mode: Mode,
) -> Result<Option<ImageSurface>, LayerRenderError> {
    let Some(dataset) = shading_data.get(country) else {
        return Ok(None);
    };

    read_rgba_from_gdal(&dataset, ctx, mode)
}

pub fn paint_surface(
    ctx: &Ctx,
    context: &Context,
    surface: &ImageSurface,
    alpha: f64,
) -> LayerRenderResult {
    context.save()?;

    #[allow(clippy::float_cmp)] // exact identity check: skip transform when scale is 1.0
    if ctx.scale != 1.0 {
        context.scale(1.0 / ctx.scale, 1.0 / ctx.scale);
    }

    context.set_source_surface(surface, 0.0, 0.0)?;

    context.paint_with_alpha(alpha)?;

    context.restore()?;

    Ok(())
}

pub fn mask_covers_tile(surfaces: &mut [&mut ImageSurface]) -> Result<bool, LayerRenderError> {
    if surfaces.is_empty() {
        return Ok(false);
    }

    let width = surfaces[0].width() as usize;
    let height = surfaces[0].height() as usize;

    if width == 0 || height == 0 {
        return Ok(false);
    }

    let mut coverage = vec![false; width * height];
    let mut remaining = coverage.len();

    for surface in surfaces {
        if surface.width() as usize != width || surface.height() as usize != height {
            return Ok(false);
        }

        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data()?;

        for y in 0..height {
            let row_start = y * stride;
            let cov_row_start = y * width;

            for x in 0..width {
                let cov_index = cov_row_start + x;

                if coverage[cov_index] {
                    continue;
                }

                let alpha = data[row_start + x * 4 + 3];

                if alpha != 0 {
                    coverage[cov_index] = true;
                    remaining -= 1;

                    if remaining == 0 {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(false)
}
