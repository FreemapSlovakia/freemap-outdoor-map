use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

/// Pier decks. `layers::roads` leaves the piers found here to us.
pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let query = "
        SELECT
            geometry
        FROM
            osm_landcovers
        WHERE
            geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857) AND
            type = 'pier'
    ";

    client
        .query(query, &ctx.bbox_query_params(None).as_params())
        .await
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("pier_areas::render");

    context.save()?;

    context.set_line_width(1.0);
    context.set_dash(&[], 0.0);

    for row in rows {
        let geometry = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        path_geometry(context, &geometry);

        context.set_source_color(colors::WHITE);
        context.fill_preserve()?;

        context.set_source_color(colors::PIER);
        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}
