use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("sea::render");

    let context = ctx.context;

    context.save()?;

    context.set_source_color(colors::WATER);
    context.paint()?;

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("sea", || {
        let table = match zoom {
            ..=7 => "land_z5_7",
            8..=10 => "land_z8_10",
            11..=13 => "land_z11_13",
            14.. => "land_z14_plus",
        };

        #[cfg_attr(any(), rustfmt::skip)]
        let sql = format!("
            SELECT
                ST_Intersection(
                    ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5),
                    ST_Buffer(geometry, $6)
                ) AS geometry
            FROM
                {table}
            WHERE
                geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)
        ");

        client.query(
            &sql,
            &ctx.bbox_query_params(Some(2.0))
                .push((20.0 - zoom as f64).exp2() / 25.0)
                .as_params(),
        )
    })?;

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        path_geometry(context, &geom);

        context.set_source_color(colors::WHITE);
        context.fill()?;
    }

    context.restore()?;

    Ok(())
}
