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
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("housenumbers::render");

    let sql = r#"
        SELECT
            COALESCE(
                NULLIF("addr:streetnumber", ''),
                NULLIF("addr:housenumber", ''),
                NULLIF("addr:conscriptionnumber", '')
            ) AS housenumber,
            geometry
        FROM osm_housenumbers
        WHERE geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY osm_id"#;

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            size: 8.0,
            ..FontAndLayoutOptions::default()
        },
        halo_opacity: 0.5,
        color: colors::AREA_LABEL,
        placements: &[0.0, 3.0, -3.0],
        ..TextOptions::default()
    };

    let rows = client.query(sql, &ctx.bbox_query_params(Some(128.0)).as_params())?;

    for row in rows {
        draw_text(
            ctx.context,
            Some(collision),
            &geometry_point(&row).project_to_tile(&ctx.tile_projector),
            row.get("housenumber"),
            &text_options,
        )?;
    }

    Ok(())
}
