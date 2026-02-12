use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("borders::render");

    let rows = ctx.legend_features("country_borders", || {
        let sql = "
            SELECT
                geometry
            FROM
                osm_country_members
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ";

        client.query(sql, &ctx.bbox_query_params(Some(10.0)).as_params())
    })?;

    ctx.context.push_group();

    let context = ctx.context;

    for row in rows {
        let geometry = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        ctx.context.set_dash(&[], 0.0);
        ctx.context.set_source_color(colors::ADMIN_BORDER);
        ctx.context.set_line_width(if ctx.zoom <= 10 {
            6.0f64.mul_add(1.4f64.powf(ctx.zoom as f64 - 11.0), 0.5)
        } else {
            6.0
        });
        ctx.context.set_line_join(cairo::LineJoin::Round);
        path_geometry(context, &geometry);
        ctx.context.stroke()?;
    }

    context.pop_group_to_source()?;
    context.paint_with_alpha(0.5)?;

    Ok(())
}
