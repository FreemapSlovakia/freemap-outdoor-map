use crate::render::{
    Feature,
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::text_on_line::{TextOnLineOptions, draw_text_on_line},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

/// Golf hole numbers, from `ref`. A hole's `name` is not drawn: mappers put
/// the number there when it labelled holes, as OSM Carto found.
pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            geometry,
            ref
        FROM
            osm_feature_lines
        WHERE
            type = 'golf_hole' AND
            ref <> '' AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            osm_id
    ";

    client
        .query(sql, &ctx.bbox_query_params(Some(512.0)).as_params())
        .await
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("golf_hole_refs::render");

    let options = TextOnLineOptions {
        color: colors::AREA_LABEL,
        ..TextOnLineOptions::default()
    };

    for row in rows {
        // legend samples of the other lines come with an empty one
        let ref_ = row.get_string("ref")?;

        if ref_.is_empty() {
            continue;
        }

        let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

        draw_text_on_line(context, &geom, ref_, Some(collision), &options)?;
    }

    Ok(())
}
