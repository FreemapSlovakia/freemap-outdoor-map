use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

/// Pier decks, from both places imposm can put them.
///
/// `area=yes` piers and multipolygon relations only reach the polygon table; a plain closed
/// way reaches both it and `osm_roads`, and before the mapping change that added `pier` to
/// `landcovers` it reached only `osm_roads` - so take the closed ways from there as well and
/// drop the ones the polygon table already has. `layers::roads` leaves closed piers to us.
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
        UNION ALL
        SELECT
            geometry
        FROM
            osm_roads
        WHERE
            geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857) AND
            type = 'pier' AND
            ST_IsClosed(geometry) AND
            ST_NPoints(geometry) > 3 AND
            NOT EXISTS (
                SELECT 1 FROM osm_landcovers
                WHERE osm_landcovers.osm_id = osm_roads.osm_id AND osm_landcovers.type = 'pier'
            )
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
