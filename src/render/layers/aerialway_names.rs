use crate::render::{
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        offset_line::offset_line_string,
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("aerialway_names::render");

    let rows = ctx.legend_features("feature_lines", || {
        let sql = "
            SELECT
                geometry,
                name
            FROM
                osm_feature_lines
            WHERE
                name <> '' AND
                type IN ('cable_car', 'chair_lift', 'drag_lift', 'gondola', 'goods', 'j-bar', 'magic_carpet', 'mixed_lift', 'platter', 'rope_tow', 't-bar', 'zip_line') AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                osm_id
        ";

        client.query(sql, &ctx.bbox_query_params(Some(512.0)).as_params())
    })?;

    let options = TextOnLineOptions {
        distribution: Distribution::Align {
            align: Align::Center,
            repeat: Repeat::Spaced(200.0),
        },
        color: colors::BLACK,
        ..TextOnLineOptions::default()
    };

    for row in rows {
        let name = row.get_string("name")?;

        let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

        let geom = offset_line_string(&geom, 10.0);

        draw_text_on_line(ctx.context, &geom, name, Some(collision), &options)?;
    }

    Ok(())
}
