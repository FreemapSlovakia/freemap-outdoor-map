use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("cutlines::render");

    let rows = ctx.legend_features("cutlines", || {
        let sql = "
            SELECT
                geometry
            FROM
                osm_feature_lines
            WHERE
                type = 'cutline' AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ";

        client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())
    })?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        let geom = row.line_string()?.project_to_tile(&ctx.tile_projector);

        path_line_string(context, &geom);

        context.set_source_color(colors::SCRUB);
        context.set_dash(&[], 0.0);
        context.set_line_width(0.33f64.mul_add(((ctx.zoom - 12) as f64).exp2(), 2.0));
        context.stroke_preserve()?;
        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}
