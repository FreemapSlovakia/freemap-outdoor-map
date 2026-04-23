use crate::render::{
    Feature,
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        text::{TextOptions, draw_text},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    regex_replacer::{Replacement, replace},
};
use cairo::Context;
use cosmic_text::Style;
use regex::Regex;
use std::sync::LazyLock;

static REPLACEMENTS: LazyLock<Vec<Replacement>> =
    LazyLock::new(|| vec![(Regex::new("[Vv]odná [Nn]ádrž").expect("regex"), "v. n.")]);

pub async fn query(ctx: &Ctx, client: &tokio_postgres::Client) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
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
    ).await
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    collision: &mut Collision,
) -> LayerRenderResult {
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

    for row in rows {
        draw_text(
            context,
            Some(collision),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            &replace(row.get_string("name")?, &REPLACEMENTS),
            &text_options,
        )?;
    }

    Ok(())
}
