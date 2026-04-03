use crate::render::{
    Feature,
    collision::Collision,
    ctx::Ctx,
    draw::text::{TextOptions, draw_text},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;
use postgres::Client;

pub fn query(ctx: &Ctx, client: &mut Client) -> Result<Vec<Feature>, postgres::Error> {
    ctx.legend_features("building_names", || {
        let sql = "
            SELECT
                osm_buildings.name,
                ST_Centroid(osm_buildings.geometry) AS geometry
            FROM osm_buildings
            LEFT JOIN osm_landcovers USING (osm_id)
            LEFT JOIN osm_pois USING (osm_id)
            LEFT JOIN osm_place_of_worships USING (osm_id)
            LEFT JOIN osm_shops USING (osm_id)
            WHERE
                osm_buildings.name <> '' AND
                osm_buildings.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                osm_buildings.type <> 'no' AND
                osm_landcovers.osm_id IS NULL AND
                osm_pois.osm_id IS NULL AND
                osm_place_of_worships.osm_id IS NULL AND
                osm_shops.osm_id IS NULL
            ORDER BY
                osm_buildings.osm_id
        ";

        client.query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
    })
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("building_names::render");

    for row in rows {
        draw_text(
            context,
            Some(collision),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            row.get_string("name")?,
            &TextOptions::default(),
        )?;
    }

    Ok(())
}
