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
            SELECT DISTINCT ON (osm_landcovers.osm_id)
                osm_landcovers.geometry,
                osm_landcovers.name,
                osm_landcovers.type IN ('forest', 'wood', 'scrub', 'heath', 'grassland', 'scree', 'blockfield', 'meadow', 'fell', 'wetland') AS natural,
                z_order,
                osm_landcovers.osm_id AS osm_id
            FROM
                osm_landcovers
            LEFT JOIN
                z_order_landuse USING (type)
            WHERE
                osm_landcovers.type NOT IN ('zoo', 'theme_park') AND
                osm_landcovers.name <> '' AND
                osm_landcovers.area >= $6 AND
                osm_landcovers.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                osm_landcovers.osm_id, osm_landcovers.type IN ('forest', 'wood', 'scrub', 'heath', 'grassland', 'scree', 'blockfield', 'meadow', 'fell', 'wetland') DESC
        )
        SELECT name, \"natural\", ST_PointOnSurface(geometry) AS geometry
        FROM lcn
        ORDER BY z_order, osm_id";

    let mut text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        color: colors::PROTECTED,
        ..TextOptions::default()
    };

    let rows = client.query(
        sql,
        &ctx.bbox_query_params(Some(512.0))
            .push(2_400_000.0f64 / (2.0f64 * (ctx.zoom as f64 - 10.0)).exp2())
            .as_params(),
    )?;

    for row in rows {
        let natural: bool = row.get("natural");

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
