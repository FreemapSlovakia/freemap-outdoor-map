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

    // nested sql is to remove duplicate entries imported by imposm because we use `mappings` in yaml
    let sql = "
        WITH lcn AS (
            SELECT DISTINCT ON (osm_landusages.osm_id)
                osm_landusages.geometry, osm_landusages.name, osm_landusages.area,
                osm_landusages.type IN ('forest', 'wood', 'scrub', 'heath', 'grassland', 'scree', 'blockfield', 'meadow', 'fell', 'wetland') AS natural,
                z_order,
                osm_landusages.osm_id AS osm_id
            FROM
                osm_landusages
            LEFT JOIN
                z_order_landuse USING (type)
            LEFT JOIN
                osm_feature_polys USING (osm_id)
            LEFT JOIN
                -- NOTE filtering some POIs (hacky because it affects also lower zooms)
                osm_sports ON osm_landusages.osm_id = osm_sports.osm_id AND osm_sports.type IN ('soccer', 'tennis', 'basketball', 'shooting')
            WHERE
                osm_landusages.name <> '' AND
                osm_feature_polys.osm_id IS NULL AND
                osm_sports.osm_id IS NULL AND
                osm_landusages.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                osm_landusages.osm_id, osm_landusages.type IN ('forest', 'wood', 'scrub', 'heath', 'grassland', 'scree', 'blockfield', 'meadow', 'fell', 'wetland') DESC
        ) SELECT name, area, \"natural\", ST_PointOnSurface(geometry) AS geometry FROM lcn ORDER BY z_order, osm_id";

    let mut text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        color: colors::PROTECTED,
        ..TextOptions::default()
    };

    let rows = client.query(sql, &ctx.bbox_query_params(Some(512.0)).as_params())?;

    for row in rows {
        let area: f32 = row.get("area");

        let natural: bool = row.get("natural");

        if area < 2_400_000.0 / (2.0 * (ctx.zoom as f32 - 10.0)).exp2() {
            continue;
        }

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
            &geometry_point(&row).project_to_tile(&ctx.tile_projector),
            &replace(row.get("name"), &REPLACEMENTS),
            &text_options,
        )?;
    }

    Ok(())
}
