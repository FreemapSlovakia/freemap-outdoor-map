use crate::render::{
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("aerialways::render");

    let rows = ctx.legend_features("aerialways", || {
        let sql = "
            SELECT
                geometry,
                type
            FROM
                osm_aerialways
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                osm_id
        ";

        client.query(sql, &ctx.bbox_query_params(Some(10.0)).as_params())
    })?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        context.set_source_rgb(0.0, 0.0, 0.0);
        context.set_dash(&[], 0.0);
        context.set_line_width(1.0);

        let geom = row.line_string()?.project_to_tile(&ctx.tile_projector);

        path_line_string(context, &geom);

        context.stroke_preserve()?;

        context.set_dash(&[1.0, 25.0], 0.0);
        context.set_line_width(5.0);

        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}
