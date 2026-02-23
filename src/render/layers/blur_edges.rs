use crate::render::{
    coverage::{
        TileCoverageRelation, edge_fade_cutoff_px, edge_fade_sigma_px, tile_touches_coverage,
    },
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::{Format, ImageSurface, Operator};
use geo::Geometry;
use image::{GrayImage, imageops};

pub fn render(ctx: &Ctx, coverage_polygon_merc: &Geometry) -> LayerRenderResult {
    let _span = tracy_client::span!("blur_edges::render");

    let context = ctx.context;

    match tile_touches_coverage(coverage_polygon_merc, ctx.bbox, ctx.meters_per_pixel()) {
        TileCoverageRelation::Crosses => {
            let pad = edge_fade_cutoff_px(ctx.meters_per_pixel()).ceil();

            let coverage_geometry = coverage_polygon_merc.project_to_tile(&ctx.tile_projector);

            let padded_w = (ctx.size.width + pad as u32 * 2) as i32;
            let padded_h = (ctx.size.height + pad as u32 * 2) as i32;

            let mut coverage_surface = ImageSurface::create(Format::A8, padded_w, padded_h)?;

            {
                let coverage_ctx = cairo::Context::new(&coverage_surface)?;

                coverage_ctx.translate(pad, pad);
                path_geometry(&coverage_ctx, &coverage_geometry);
                coverage_ctx.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                coverage_ctx.fill()?;
                coverage_surface.flush();
            }

            let coverage_width = coverage_surface.width() as u32;
            let coverage_height = coverage_surface.height() as u32;
            let stride = coverage_surface.stride() as usize;

            let alpha: Vec<u8> = {
                let data = coverage_surface.data().expect("surface data");

                data.chunks(stride)
                    .take(coverage_height as usize)
                    .flat_map(|row| row.iter().copied().take(coverage_width as usize))
                    .collect()
            };

            let gray = GrayImage::from_vec(coverage_width, coverage_height, alpha)
                .expect("valid coverage alpha buffer");

            let blurred =
                imageops::blur(&gray, edge_fade_sigma_px(ctx.meters_per_pixel()) as f32).into_raw();

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
                (coverage_width * 4) as i32,
            )?;

            context.set_source_surface(&blurred_surface, -pad, -pad)?;
        }
        TileCoverageRelation::Inside => {
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        }
        TileCoverageRelation::Outside => {
            context.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        }
    }

    context.set_operator(Operator::DestIn);
    context.paint()?;

    Ok(())
}
