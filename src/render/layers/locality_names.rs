use crate::render::{
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
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("locality_names::render");

    let rows = ctx.legend_features("locality_names", || {
        let sql = "
            SELECT
                name,
                geometry
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

        client.query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
    })?;

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
            ctx.context,
            Some(collision),
            &row.point()?.project_to_tile(&ctx.tile_projector),
            row.get_string("name")?,
            &text_options,
        )?;
    }

    Ok(())
}
