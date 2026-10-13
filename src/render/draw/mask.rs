use crate::render::{
    ctx::Ctx,
    layer_render_error::{LayerRenderError, LayerRenderResult},
};
use cairo::{Context, Format, ImageSurface, SurfacePattern};

/// A tile-sized image at `ctx.scale`, drawn by `draw`. Masks are combined on
/// these rather than on the target: the PDF backend cannot express `In`, `Out`
/// or `DestOut` and rasterizes the whole page for them.
///
/// Rounded up, so on a vector page a fractional `size × scale` leaves no strip
/// uncovered.
pub fn compose(
    ctx: &Ctx,
    draw: impl FnOnce(&Context) -> LayerRenderResult,
) -> Result<ImageSurface, LayerRenderError> {
    let surface = ImageSurface::create(
        Format::ARgb32,
        (ctx.size.width as f64 * ctx.scale).ceil() as i32,
        (ctx.size.height as f64 * ctx.scale).ceil() as i32,
    )?;

    draw(&Context::new(&surface)?)?;

    Ok(surface)
}

/// Paints a tile-resolution `surface` over the whole of a [`compose`] image.
pub fn paint_whole(cc: &Context, surface: &ImageSurface, alpha: f64) -> LayerRenderResult {
    cc.set_source_surface(surface, 0.0, 0.0)?;
    cc.paint_with_alpha(alpha)?;

    Ok(())
}

/// Paints the current source through the alpha of a tile-resolution `mask`.
pub fn mask_with(ctx: &Ctx, context: &Context, mask: &ImageSurface) -> LayerRenderResult {
    let pattern = SurfacePattern::create(mask);
    pattern.set_matrix(cairo::Matrix::new(ctx.scale, 0.0, 0.0, ctx.scale, 0.0, 0.0));

    context.mask(&pattern)?;

    Ok(())
}
