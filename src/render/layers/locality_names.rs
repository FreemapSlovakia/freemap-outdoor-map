use crate::render::{
    Feature,
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        text::{TextOptions, draw_text},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            name,
            ST_PointOnSurface(geometry) AS geometry
        FROM
            osm_places
        WHERE
            name <> '' AND
            type IN ('locality', 'city_block', 'plot') AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            z_order DESC,
            population DESC,
            osm_id
    ";

    client
        .query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
        .await
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("locality_names::render");

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            size: 11.0,
            ..FontAndLayoutOptions::default()
        },
        halo_opacity: 0.2,
        color: colors::LOCALITY_LABEL,
        ..TextOptions::default()
    };

    for row in rows {
        draw_text(
            context,
            Some(collision),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            row.get_string("name")?,
            &text_options,
        )?;
    }

    Ok(())
}
