use crate::render::{
    ctx::Ctx,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::{hillshading_datasets::HillshadingDatasets, hillshading_footprint::PixelWindow},
};
use cairo::{Context, Format, ImageSurface};
use gdal::Dataset;

pub enum Mode {
    Mask,
    Shading,
}

/// In-place morphological erosion of a 0/255 mask by a rectangular structuring
/// element (separable min-filter, independent radius per axis). Shrinks the
/// valid region so output pixels whose resampling footprint could reach a masked
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
    let size = ctx.size;

    // The same mapping the footprint test uses, so a tile can never be accepted by one
    // and read at a different window by the other.
    let window = PixelWindow::new(&dataset.geo_transform()?, &ctx.bbox);

    let (pixel_min_x_f, pixel_max_x_f) = (window.min.x, window.max.x);
    let (pixel_min_y_f, pixel_max_y_f) = (window.min.y, window.max.y);

    let (pixel_min_x, pixel_max_x) = (window.min_x(), window.max_x());
    let (pixel_min_y, pixel_max_y) = (window.min_y(), window.max_y());

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

    // From the unclamped window, so a tile barely overlapping the raster edge
    // doesn't get a ceil-distorted ratio and a different kernel than its neighbour.
    let ratio_x = buffered_w as f64 / window_width_px as f64;
    let ratio_y = buffered_h as f64 / window_height_px as f64;
    let ratio = ratio_x.max(ratio_y);

    // A reduction that is (near) a power of two is served from an overview at
    // ~1:1, and gdaladdo already box-filtered those overviews, so Nearest picks
    // the right pixel without aliasing — 200x cheaper and needs no erosion.
    // Ratios in between (scale 3 gives 4/3, 8/3, 16/3) land between overview
    // levels, where Nearest really would point-sample. The tolerance applies on
    // both sides of 1:1, as ceil noise in buffered_w can push it slightly above.
    let served_by_overview = {
        let l = (1.0 / ratio).log2();
        l > -0.02 && (l - l.round()).abs() < 0.02
    };
    let upscaling = !served_by_overview && ratio > 1.0;

    // CubicSpline up: Lanczos' negative lobes cancel the blur that minifying
    // introduces, but magnifying has no blur to cancel and they only overshoot —
    // 18% of pixels at 8x fall outside their source neighbourhood's range, and
    // the result is sharper than nearest. Cubic for the leftovers: same support
    // as CubicSpline so the erosion radius stays 2, and ~20% faster than Lanczos.
    let resample_alg = if served_by_overview {
        gdal::raster::ResampleAlg::NearestNeighbour
    } else if upscaling {
        gdal::raster::ResampleAlg::CubicSpline
    } else {
        gdal::raster::ResampleAlg::Cubic
    };

    // Erosion radius follows the kernel actually used: Nearest reads one source
    // pixel so it cannot pull in a masked one, Cubic and CubicSpline both reach 2.
    // Expressed in output pixels — GDAL scales the kernel to the output spacing
    // when minifying, so the footprint stays ~2 there, and becomes 2 * ratio when
    // magnifying.
    let erode_radius = if served_by_overview { 0.0 } else { 2.0 };

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
                    Some(resample_alg),
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

    // Read the per-dataset mask with NearestNeighbour (no ringing in the mask
    // itself), then erode by the radius the chosen kernel actually needs — see
    // erode_radius above. Zero when reading straight from an overview, which
    // gives back the 2 px of coastline the old fixed radius ate at every zoom
    // below native.
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

        let radius_x = (erode_radius * ratio_x.max(1.0)).ceil() as usize;
        let radius_y = (erode_radius * ratio_y.max(1.0)).ceil() as usize;

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
            Some(resample_alg),
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
    let Some(dataset) = shading_data.get(country, &ctx.bbox) else {
        return Ok(None);
    };

    let what = match mode {
        Mode::Mask => "mask",
        Mode::Shading => "shading",
    };

    let started = std::time::Instant::now();

    let surface = read_rgba_from_gdal(&dataset, ctx, mode);

    let took = started.elapsed();

    if took >= SLOW_READ {
        let (x, y) = tile_at_center(ctx);

        eprintln!(
            "hillshading {country}: {what} read took {:.1}s at {}/{x}/{y} scale {}",
            took.as_secs_f64(),
            ctx.zoom,
            ctx.scale
        );
    }

    surface
}

/// A single raster read this slow is logged with its tile, so a hanging tile can be reproduced.
const SLOW_READ: std::time::Duration = std::time::Duration::from_secs(5);

fn tile_at_center(ctx: &Ctx) -> (u32, u32) {
    const HALF_WORLD: f64 = 20_037_508.342_789_244;

    let center = ctx.bbox.center();
    let n = f64::from(1u32 << ctx.zoom);

    (
        ((center.x + HALF_WORLD) / (2.0 * HALF_WORLD) * n).floor() as u32,
        ((HALF_WORLD - center.y) / (2.0 * HALF_WORLD) * n).floor() as u32,
    )
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

/// Which tile pixels a surface has a non-zero alpha at, one bit each.
///
/// Both what covers the tile and which dataset was credited for a pixel come out
/// of the same question — "is anything painted here" — so both are answered from
/// these, with intersection and subtraction a word at a time. 8 KiB per surface
/// for a 256 px tile.
pub struct MaskBits {
    words: Vec<u64>,
    pixels: usize,
}

impl MaskBits {
    pub fn from_surface(surface: &mut ImageSurface) -> Result<Self, LayerRenderError> {
        let width = surface.width() as usize;
        let height = surface.height() as usize;
        let pixels = width * height;

        let mut words = vec![0u64; pixels.div_ceil(64)];

        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data()?;

        for y in 0..height {
            let row_start = y * stride;
            let bit_row_start = y * width;

            for x in 0..width {
                if data[row_start + x * 4 + 3] != 0 {
                    let bit = bit_row_start + x;
                    words[bit / 64] |= 1 << (bit % 64);
                }
            }
        }

        Ok(Self { words, pixels })
    }

    /// Every pixel of the tile is painted.
    pub fn is_full(&self) -> bool {
        self.pixels != 0
            && self
                .words
                .iter()
                .enumerate()
                .all(|(i, word)| *word == self.full_word(i))
    }

    /// Any pixel painted here and by none of `minus` — i.e. whether this surface
    /// survives the `DestOut` subtraction of the better-priority masks and so
    /// really did contribute to the tile.
    pub fn has_any_outside(&self, minus: &[&Self]) -> bool {
        self.words.iter().enumerate().any(|(i, word)| {
            let cut = minus
                .iter()
                .filter(|other| other.pixels == self.pixels)
                .fold(0u64, |acc, other| acc | other.words[i]);

            word & !cut != 0
        })
    }

    /// Any pixel painted in both this and `other`, and by none of `minus`. A mask
    /// says the DEM has data there; the shading surface's own alpha says whether
    /// that data drew anything, and only the intersection was really contributed.
    pub fn has_any_shared_outside(&self, other: &Self, minus: &[&Self]) -> bool {
        if other.pixels != self.pixels {
            return self.has_any_outside(minus);
        }

        self.words.iter().enumerate().any(|(i, word)| {
            let cut = minus
                .iter()
                .filter(|other| other.pixels == self.pixels)
                .fold(0u64, |acc, other| acc | other.words[i]);

            word & other.words[i] & !cut != 0
        })
    }

    /// The union, or `None` when there is nothing to unite or the surfaces disagree
    /// on size (which would make the bit indices mean different pixels).
    pub fn union<'a>(masks: impl IntoIterator<Item = &'a Self>) -> Option<Self> {
        let mut masks = masks.into_iter();
        let first = masks.next()?;

        let mut union = Self {
            words: first.words.clone(),
            pixels: first.pixels,
        };

        for mask in masks {
            if mask.pixels != union.pixels {
                return None;
            }

            for (word, other) in union.words.iter_mut().zip(&mask.words) {
                *word |= other;
            }
        }

        Some(union)
    }

    /// The bits of word `i` that correspond to real pixels; the last word of a tile
    /// whose pixel count is not a multiple of 64 is only partly used.
    fn full_word(&self, i: usize) -> u64 {
        let used = (self.pixels - i * 64).min(64);

        if used == 64 { u64::MAX } else { (1 << used) - 1 }
    }
}

pub fn mask_covers_tile(surfaces: &mut [&mut ImageSurface]) -> Result<bool, LayerRenderError> {
    let mut masks = Vec::with_capacity(surfaces.len());

    for surface in surfaces {
        masks.push(MaskBits::from_surface(surface)?);
    }

    Ok(MaskBits::union(&masks).is_some_and(|union| union.is_full()))
}

#[cfg(test)]
mod mask_bits_tests {
    use super::MaskBits;
    use cairo::{Format, ImageSurface};

    /// A surface whose alpha is non-zero exactly where `painted` says.
    fn surface(width: i32, height: i32, painted: impl Fn(i32, i32) -> bool) -> ImageSurface {
        let mut surface = ImageSurface::create(Format::ARgb32, width, height).expect("surface");

        {
            let stride = surface.stride() as usize;
            let mut data = surface.data().expect("surface data");

            for y in 0..height {
                for x in 0..width {
                    if painted(x, y) {
                        data[y as usize * stride + x as usize * 4 + 3] = 255;
                    }
                }
            }
        }

        surface
    }

    fn bits(width: i32, height: i32, painted: impl Fn(i32, i32) -> bool) -> MaskBits {
        MaskBits::from_surface(&mut surface(width, height, painted)).expect("bits")
    }

    #[test]
    fn a_tile_is_full_only_when_every_pixel_is_painted() {
        // 8x8 is exactly one word; 9x9 is 81 bits, so the last word is part used and
        // its unused bits must not be read as unpainted.
        for (w, h) in [(8, 8), (9, 9), (16, 4), (13, 7)] {
            assert!(bits(w, h, |_, _| true).is_full(), "{w}x{h} all painted");
            assert!(!bits(w, h, |_, _| false).is_full(), "{w}x{h} none painted");

            // One hole anywhere, including in the last partial word.
            let last = w * h - 1;
            assert!(
                !bits(w, h, |x, y| y * w + x != last).is_full(),
                "{w}x{h} missing its last pixel"
            );
            assert!(
                !bits(w, h, |x, y| y * w + x != 0).is_full(),
                "{w}x{h} missing its first pixel"
            );
        }

        // Cairo will not make a zero-size surface, so build the degenerate case by
        // hand: it must cover nothing rather than vacuously everything.
        assert!(
            !MaskBits {
                words: Vec::new(),
                pixels: 0
            }
            .is_full()
        );
    }

    #[test]
    fn a_dataset_survives_only_where_no_better_one_covers_it() {
        // Left half painted, and a better dataset covering the left quarter.
        let mask = bits(16, 4, |x, _| x < 8);
        let better = bits(16, 4, |x, _| x < 4);
        let covering = bits(16, 4, |x, _| x < 8);

        assert!(mask.has_any_outside(&[]));
        assert!(mask.has_any_outside(&[&better]));
        // Fully subtracted: nothing of it reaches the tile.
        assert!(!mask.has_any_outside(&[&covering]));
        // Several subtractions combine.
        let right = bits(16, 4, |x, _| (4..8).contains(&x));
        assert!(!mask.has_any_outside(&[&better, &right]));
    }

    #[test]
    fn credit_needs_the_mask_and_the_shading_to_overlap() {
        let mask = bits(16, 4, |x, _| x < 8);

        // The DEM has data across the left half but drew only in its right part.
        let painted = bits(16, 4, |x, _| (6..12).contains(&x));
        assert!(mask.has_any_shared_outside(&painted, &[]));

        // …and a better dataset takes exactly that part away.
        let better = bits(16, 4, |x, _| (6..8).contains(&x));
        assert!(!mask.has_any_shared_outside(&painted, &[&better]));

        // Data everywhere, drawn nowhere.
        let blank = bits(16, 4, |_, _| false);
        assert!(!mask.has_any_shared_outside(&blank, &[]));

        // Drawn only outside the mask.
        let elsewhere = bits(16, 4, |x, _| x >= 8);
        assert!(!mask.has_any_shared_outside(&elsewhere, &[]));
    }

    #[test]
    fn the_union_needs_the_masks_to_agree_on_size() {
        let left = bits(16, 4, |x, _| x < 8);
        let right = bits(16, 4, |x, _| x >= 8);

        assert!(MaskBits::union([&left, &right]).expect("union").is_full());
        assert!(!MaskBits::union([&left]).expect("union").is_full());
        assert!(MaskBits::union([]).is_none());

        // Different pixel counts mean the bit indices are different pixels, so the
        // union would be nonsense rather than merely wrong.
        let other = bits(8, 9, |_, _| true);
        assert!(MaskBits::union([&left, &other]).is_none());
    }
}
