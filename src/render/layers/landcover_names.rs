use super::landcover_z_order::build_landcover_z_order_case;
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
use std::sync::LazyLock;

static REPLACEMENTS: LazyLock<Vec<Replacement>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r"[Čč]istička odpadových vôd").expect("regex"),
            "ČOV",
        ),
        (
            Regex::new(r"[Pp]oľnohospodárske družstvo").expect("regex"),
            "PD",
        ),
        (Regex::new(r"[Nn]ámestie").expect("regex"), "nám. "),
    ]
});

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("landcover_names::render");

    let rows = ctx.legend_features("landcover_names", || {
        let z_order_case = build_landcover_z_order_case("type");

        // TODO include types (`type IN`), don't exclude (`type NOT IN`)
        // TODO ... or maybe merge with bordered_area_names
        // nested sql is to remove duplicate entries imported by imposm because we use `mappings` in yaml
        let sql = format!(
            "
            WITH main AS (
                SELECT DISTINCT ON (osm_id)
                    geometry,
                    name,
                    type,
                    osm_id AS osm_id
                FROM
                    osm_landcovers
                WHERE
                    type NOT IN ('zoo', 'theme_park', 'winter_sports') AND
                    name <> '' AND
                    area >= $6 AND
                    geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            )
            SELECT
                name,
                type IN ('forest', 'wood', 'scrub', 'heath', 'grassland', 'scree', 'blockfield', 'meadow', 'fell', 'wetland') AS natural,
                ST_PointOnSurface(geometry) AS geometry
            FROM
                main
            ORDER BY
                {z_order_case} DESC,
                osm_id
            ",
        );

        client.query(
            &sql,
            &ctx.bbox_query_params(Some(512.0))
                .push(2_400_000.0f32 / (2.0f32 * (ctx.zoom as f32 - 10.0)).exp2())
                .as_params(),
        )
    })?;

    let mut text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        color: colors::PROTECTED,
        ..TextOptions::default()
    };

    for row in rows {
        let natural = row.get_bool("natural")?;

        text_options.flo.style = if natural {
            Style::Italic
        } else {
            Style::Normal
        };

        text_options.color = if natural {
            colors::PROTECTED
        } else {
            colors::AREA_LABEL
        };

        draw_text(
            ctx.context,
            Some(collision),
            &row.point()?.project_to_tile(&ctx.tile_projector),
            &replace(row.get_string("name")?, &REPLACEMENTS),
            &text_options,
        )?;
    }

    Ok(())
}
