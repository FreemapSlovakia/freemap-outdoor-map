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
    let _span = tracy_client::span!("water_area_names::render");

    let sql = "
        SELECT
            REGEXP_REPLACE(osm_waterareas.name, '[Vv]odná [Nn]ádrž\\M', 'v. n.') AS name,
            ST_PointOnSurface(osm_waterareas.geometry) AS geometry
        FROM
            osm_waterareas LEFT JOIN osm_feature_polys USING (osm_id)
        WHERE
            osm_waterareas.name <> '' AND
            osm_waterareas.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            osm_feature_polys.osm_id IS NULL AND
            osm_waterareas.type <> 'riverbank' AND
            osm_waterareas.water NOT IN ('river', 'stream', 'canal', 'ditch') AND
            ($6 >= 17 OR osm_waterareas.area > 800000 / POWER(2, (2 * ($6 - 10))))
        ";

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        color: colors::WATER_LABEL,
        halo_color: colors::WATER_LABEL_HALO,
        ..TextOptions::default()
    };

    let mut params = ctx.bbox_query_params(Some(1024.0));
    params.push(ctx.zoom as i32);

    let rows = client.query(sql, &params.as_params())?;

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
