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
    regex_replacer::{Replacement, replace},
};
use pangocairo::pango::Style;
use postgres::Client;
use regex::Regex;
use std::sync::LazyLock;

pub static REPLACEMENTS: LazyLock<Vec<Replacement>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r"\b[Oo]chranné [Pp]ásmo\b").expect("regex"),
            "OP",
        ),
        (
            Regex::new(r"\b[Nn]árodn(ého|ý) [Pp]arku?\b").expect("regex"),
            "NP",
        ),
    ]
});

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("national_park_names::render");

    let sql = "
        SELECT type, name, protect_class, ST_PointOnSurface(geometry) AS geometry
        FROM osm_protected_areas
        WHERE
            name <> '' AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            (type = 'national_park' OR (type = 'protected_area' AND protect_class = '2'))
        ORDER BY name LIKE ('Ochranné pásmo %'), area DESC";

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            size: 9.0 + (ctx.zoom as f64 - 7.0).exp2(),
            ..FontAndLayoutOptions::default()
        },
        color: colors::PROTECTED,
        ..TextOptions::default()
    };

    let rows = client.query(sql, &ctx.bbox_query_params(Some(512.0)).as_params())?;

    for row in rows {
        draw_text(
            ctx.context,
            Some(collision),
            &geometry_point(&row).project_to_tile(&ctx.tile_projector),
            &replace(row.get("name"), &REPLACEMENTS),
            &text_options,
        )?;
    }

    Ok(())
}
