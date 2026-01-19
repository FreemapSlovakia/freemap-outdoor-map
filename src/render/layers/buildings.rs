use crate::render::{
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("buildings::render");

    let sql = concat!(
        "SELECT type, geometry FROM osm_buildings ",
        "WHERE geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)"
    );

    let rows = client.query(sql, &ctx.bbox_query_params(None).as_params())?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        let Some(geom) =
            geometry_geometry(&row).map(|geom| geom.project_to_tile(&ctx.tile_projector))
        else {
            continue;
        };

        context.set_source_rgb(0.5, 0.5, 0.5);

        path_geometry(context, &geom);

        context.fill()?;
    }

    context.restore()?;

    Ok(())
}
