use crate::render::{
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        text::{TextOptions, draw_text},
    },
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_point},
};
use pangocairo::pango::Style;
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("special_park_names::render");

    // TODO consired area
    // TODO maybe move to landcover_names.rs

    let sql = "
        SELECT name, geometry AS geometry
        FROM osm_features
        WHERE
            name <> '' AND
            (type = 'zoo' OR type = 'theme_park') AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY osm_id";

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Normal,
            size: 11.0 + (ctx.zoom as f64 * 0.75 - 10.0).exp2(),
            ..FontAndLayoutOptions::default()
        },
        color: colors::SPECIAL_PARK,
        ..TextOptions::default()
    };

    let rows = client.query(sql, &ctx.bbox_query_params(Some(512.0)).as_params())?;

    for row in rows {
        draw_text(
            ctx.context,
            Some(collision),
            &geometry_point(&row).project_to_tile(&ctx.tile_projector),
            row.get("name"),
            &text_options,
        )?;
    }

    Ok(())
}
