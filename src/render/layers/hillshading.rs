use crate::render::{
    ctx::Ctx,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::hillshading_pool::HillshadingPool,
};
use cairo::{Format, ImageSurface};

pub enum Mode {
    Mask,
    Shading,
}

pub fn load_surface(
    ctx: &Ctx,
    country: &str,
    pool: &HillshadingPool,
    mode: Mode,
) -> Result<Option<ImageSurface>, LayerRenderError> {
    let raw = pool.read(country, ctx.bbox, ctx.size, ctx.scale, mode)?;

    let Some(raw) = raw else {
        return Ok(None);
    };

    let surface = ImageSurface::create_for_data(
        raw.data,
        Format::ARgb32,
        raw.width,
        raw.height,
        raw.stride,
    )?;

    Ok(Some(surface))
}

pub fn paint_surface(ctx: &Ctx, surface: &ImageSurface, alpha: f64) -> LayerRenderResult {
    let context = ctx.context;

    context.save()?;

    if ctx.scale != 1.0 {
        context.scale(1.0 / ctx.scale, 1.0 / ctx.scale);
    }

    context.set_source_surface(surface, 0.0, 0.0)?;

    context.paint_with_alpha(alpha)?;

    context.restore()?;

    Ok(())
}

pub fn mask_covers_tile(surfaces: &mut [ImageSurface]) -> Result<bool, LayerRenderError> {
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

pub fn load_and_paint(
    ctx: &Ctx,
    country: &str,
    alpha: f64,
    pool: &HillshadingPool,
    mode: Mode,
) -> Result<bool, LayerRenderError> {
    let surface = load_surface(ctx, country, pool, mode)?;

    if let Some(surface) = surface.as_ref() {
        paint_surface(ctx, surface, alpha)?;
    }

    Ok(surface.is_some())
}
