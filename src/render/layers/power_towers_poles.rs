use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;
use postgres::Client;

pub fn query(ctx: &Ctx, client: &mut Client) -> Result<Vec<postgres::Row>, postgres::Error> {
    let by_zoom = if ctx.zoom < 15 {
        ""
    } else {
        ", 'pylon', 'pole'"
    };

    #[cfg_attr(any(), rustfmt::skip)]
    let sql = format!("
        SELECT
            geometry,
            type
        FROM
            osm_pois
        WHERE
            type IN ('power_tower'{by_zoom}) AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
    ");

    client.query(&sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("power_lines::render_towers_poles");

    context.save()?;

    for row in rows {
        context.set_source_color(if row.get_string("type")? == "pole" {
            colors::POWER_LINE_MINOR
        } else {
            colors::POWER_LINE
        });

        let p = row.get_point()?.project_to_tile(&ctx.tile_projector);

        context.rectangle(
            ctx.hint(p.x() - 1.5),
            ctx.hint(p.y() - 1.5),
            ctx.hint(3.0),
            ctx.hint(3.0),
        );

        context.fill()?;
    }

    context.restore()?;

    Ok(())
}
