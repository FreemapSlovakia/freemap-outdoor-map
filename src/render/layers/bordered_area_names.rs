use crate::render::{
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        path_geom::walk_geometry_line_strings,
        text::{TextOptions, draw_text},
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    layers::national_park_names::REPLACEMENTS,
    projectable::TileProjectable,
    regex_replacer::replace,
};
use pangocairo::pango::Style;
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("protected_area_names::render");

    let rows = ctx.legend_features("osm_protected_areas", || {
        let sql = "
            SELECT
                name,
                ST_Centroid(geometry) AS geometry
            FROM
                osm_protected_areas
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                (type = 'nature_reserve' OR (type = 'protected_area' AND protect_class <> '2'))
            ORDER BY
                area DESC
        ";

        client.query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
    })?;

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        halo_opacity: 0.75,
        color: colors::PROTECTED,
        ..TextOptions::default()
    };

    for row in rows {
        draw_text(
            ctx.context,
            Some(collision),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            row.get_string("name")?,
            &text_options,
        )?;
    }

    let rows = ctx.legend_features("osm_landcovers", || {

            let sql = "
                WITH merged AS (
                    SELECT
                        type, name, geometry, area
                    FROM
                        osm_protected_areas
                    WHERE
                        (type = 'national_park' OR (type = 'protected_area' AND protect_class = '2'))

                    UNION ALL

                    SELECT
                        type, name, geometry, area
                    FROM
                        osm_landcovers
                    WHERE
                        type = 'winter_sports'
                )
                SELECT
                    type, name, ST_Boundary(geometry) AS geometry, area
                FROM
                    merged
                WHERE
                    name <> '' AND
                    geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
                ORDER BY
                    area DESC";

        client.query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
    })?;

    let mut text_options = TextOnLineOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        alpha: 0.66,
        halo_opacity: 0.75,
        offset: -14.0,
        distribution: Distribution::Align {
            align: Align::Center,
            repeat: Repeat::Spaced(600.0),
        },
        keep_offset_side: true,
        ..TextOnLineOptions::default()
    };

    for row in rows {
        text_options.color = match row.get_string("type")? {
            "national_park" | "protected_area" => colors::PROTECTED,
            "winter_sports" => colors::WATER,
            _ => colors::BLACK,
        };

        let name = row.get_string("name")?;

        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        walk_geometry_line_strings(&geom, &mut |geom| {
            let _drawn = draw_text_on_line(
                ctx.context,
                geom,
                &replace(name, &REPLACEMENTS),
                Some(collision),
                &text_options,
            )?;

            cairo::Result::Ok(())
        })?;
    }

    Ok(())
}
