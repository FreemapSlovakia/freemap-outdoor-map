use crate::render::{
    ctx::Ctx, draw::path_geom::path_geometry, layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("buildings::render");

    let rows = ctx.legend_features("buildings", || {
        let sql = "
            SELECT
                type,
                geometry FROM osm_buildings
            WHERE
                geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)
        ";

        client.query(sql, &ctx.bbox_query_params(None).as_params())
    })?;

    let context = ctx.context;

    context.save()?;

    context.set_source_rgb(0.5, 0.5, 0.5);

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        path_geometry(context, &geom);

        context.fill()?;
    }

    context.restore()?;

    Ok(())
}
