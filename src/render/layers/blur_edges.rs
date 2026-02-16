use crate::render::{
    ctx::Ctx, draw::path_geom::path_geometry, layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::{Format, ImageSurface, Operator};
use geo::{BoundingRect, Geometry, Intersects, Rect};
use image::{GrayImage, imageops};

const BLUR_RADIUS_PX: f64 = 10.0;

pub fn render(ctx: &Ctx, mask_polygon_merc: &Geometry) -> LayerRenderResult {
    let _span = tracy_client::span!("blur_edges::render");

    let context = ctx.context;

    if tile_intersects_mask(&mask_polygon_merc, ctx) {
        let pad = (BLUR_RADIUS_PX * 3.0).ceil() as u32;

        let mask_geometry = mask_polygon_merc.project_to_tile(&ctx.tile_projector);

        let padded_w = (ctx.size.width + pad * 2) as i32;
        let padded_h = (ctx.size.height + pad * 2) as i32;

        let mut mask_surface = ImageSurface::create(Format::A8, padded_w, padded_h)?;

        {
            let mask_ctx = cairo::Context::new(&mask_surface)?;

            mask_ctx.translate(pad as f64, pad as f64);
            path_geometry(&mask_ctx, &mask_geometry);
            mask_ctx.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            mask_ctx.fill()?;
            mask_surface.flush();
        }

        let mask_width = mask_surface.width() as u32;
        let mask_height = mask_surface.height() as u32;
        let stride = mask_surface.stride() as usize;

        let alpha: Vec<u8> = {
            let data = mask_surface.data().expect("surface data");

            data.chunks(stride)
                .take(mask_height as usize)
                .flat_map(|row| row.iter().copied().take(mask_width as usize))
                .collect()
        };

        let gray =
            GrayImage::from_vec(mask_width, mask_height, alpha).expect("valid mask alpha buffer");

        let blurred = imageops::blur(&gray, BLUR_RADIUS_PX as f32).into_raw();

        let mut blurred_rgba = vec![0u8; blurred.len() * 4];

        for (i, alpha) in blurred.iter().enumerate() {
            let idx = i * 4;
            blurred_rgba[idx + 3] = *alpha;
        }

        let blurred_surface = ImageSurface::create_for_data(
            blurred_rgba,
            Format::ARgb32,
            padded_w,
            padded_h,
            (mask_width * 4) as i32,
        )?;

        context.set_source_surface(&blurred_surface, -(pad as f64), -(pad as f64))?;
    } else {
        context.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    }

    context.set_operator(Operator::DestIn);
    context.paint()?;

    Ok(())
}

fn tile_intersects_mask(mask: &Geometry, ctx: &Ctx) -> bool {
    let Some(bbox) = mask.bounding_rect() else {
        return false;
    };

    let blur_radius_m = BLUR_RADIUS_PX * ctx.meters_per_pixel() * 3.0;

    Rect::new(
        (bbox.min().x - blur_radius_m, bbox.min().y - blur_radius_m),
        (bbox.max().x + blur_radius_m, bbox.max().y + blur_radius_m),
    )
    .intersects(&ctx.bbox)
}
