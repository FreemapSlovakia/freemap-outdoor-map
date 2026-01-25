use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{hatch::hatch_geometry, path_geom::path_geometry},
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("military_areas::render");

    let sql = "
        SELECT geometry
            FROM osm_landcovers
            WHERE
                type = 'military'
                AND geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
                AND area / POWER(4, 19 - $6) > 10";

    let rows = &client.query(
        sql,
        &ctx.bbox_query_params(Some(10.0))
            .push(ctx.zoom as i32)
            .as_params(),
    )?;

    ctx.context.push_group();

    ctx.context.push_group();

    let geometries: Vec<_> = rows
        .iter()
        .filter_map(geometry_geometry)
        .map(|geom| (geom.project_to_tile(&ctx.tile_projector), geom))
        .collect();

    let context = ctx.context;

    // hatching
    for (projected, unprojected) in &geometries {
        ctx.context.push_group();

        path_geometry(context, projected);

        context.clip();

        ctx.context.set_source_color(colors::MILITARY);
        ctx.context.set_dash(&[], 0.0);
        ctx.context.set_line_width(1.5);

        hatch_geometry(ctx, unprojected, 10.0, -45.0)?;

        ctx.context.stroke()?;

        context.pop_group_to_source()?;
        context.paint()?;
    }

    context.pop_group_to_source()?;
    context.paint_with_alpha(if ctx.zoom < 14 { 0.5 / 0.8 } else { 0.2 / 0.8 })?;

    // border

    for (projected, _) in &geometries {
        ctx.context.set_source_color(colors::MILITARY);
        ctx.context.set_dash(&[25.0, 7.0], 0.0);
        ctx.context.set_line_width(3.0);
        path_geometry(context, projected);
        ctx.context.stroke()?;
    }

    ctx.context.pop_group_to_source()?;

    ctx.context.paint_with_alpha(0.8)?;

    Ok(())
}
