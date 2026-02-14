use std::sync::LazyLock;

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
    regex_replacer::{Replacement, replace},
};
use pangocairo::pango::Style;
use postgres::Client;
use regex::Regex;

static REPLACEMENTS: LazyLock<Vec<Replacement>> =
    LazyLock::new(|| vec![(Regex::new("[Vv]odná [Nn]ádrž").expect("regex"), "v. n.")]);

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("water_area_names::render");

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        color: colors::WATER_LABEL,
        halo_color: colors::WATER_LABEL_HALO,
        ..TextOptions::default()
    };

    let rows = ctx.legend_features("water_areas", || {
        let sql = "
            SELECT
                name,
                ST_PointOnSurface(osm_waterareas.geometry) AS geometry
            FROM
                osm_waterareas
            WHERE
                osm_waterareas.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                osm_waterareas.name <> '' AND
                osm_waterareas.type <> 'riverbank' AND
                osm_waterareas.water NOT IN ('river', 'stream', 'canal', 'ditch') AND
                ($6 >= 17 OR osm_waterareas.area > 800000 / POWER(2, (2 * ($6 - 10))))
            ";

        client.query(
            sql,
            &ctx.bbox_query_params(Some(1024.0))
                .push(ctx.zoom as i32)
                .as_params(),
        )
    })?;

    for row in rows {
        draw_text(
            ctx.context,
            Some(collision),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            &replace(row.get_string("name")?, &REPLACEMENTS),
            &text_options,
        )?;
    }

    Ok(())
}
