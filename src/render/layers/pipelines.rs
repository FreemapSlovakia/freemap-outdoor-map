use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("pipelines::render");

    let rows = ctx.legend_features("pipelines", || {
        let by_zoom = if ctx.zoom < 15 {
            "'overground', 'overhead', ''"
        } else {
            "'overground', 'overhead', '', 'underground', 'underwater'"
        };

        let sql = format!(
            "
            SELECT
                geometry,
                location IN('underground', 'underwater') AS below
            FROM
                osm_pipelines
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                location IN ({by_zoom})
            ",
        );

        client.query(&sql, &ctx.bbox_query_params(Some(8.0)).as_params())
    })?;

    let context = ctx.context;

    for row in rows {
        context.push_group();

        let geom = row.line_string()?.project_to_tile(&ctx.tile_projector);

        path_line_string(context, &geom);

        context.set_source_color(colors::PIPELINE);
        context.set_dash(&[], 0.0);
        context.set_line_join(cairo::LineJoin::Round);
        context.set_line_width(2.0);

        context.stroke_preserve()?;

        context.set_line_width(4.0);
        context.set_dash(&[0.0, 15.0, 1.5, 1.5, 1.5, 1.0], 0.0);

        context.stroke()?;

        context.pop_group_to_source()?;

        context.paint_with_alpha(if row.get_bool("below")? { 0.33 } else { 1.0 })?;
    }

    Ok(())
}
