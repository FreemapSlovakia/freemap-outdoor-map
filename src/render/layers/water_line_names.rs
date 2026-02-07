use crate::render::{
    collision::Collision,
    colors::{self},
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        path_geom::walk_geometry_line_strings,
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
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
        (Regex::new(r"\b[Pp]otok$").expect("regex"), "p."),
        (Regex::new(r"^[Pp]otok\b *").expect("regex"), ""),
    ]
});

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("water_line_names::render");

    let rows = ctx.legend_features("water_lines", || {
        let w = if ctx.zoom < 14 {
            "AND type = 'river'"
        } else {
            ""
        };

        let sql = format!("
            WITH merged AS (
                SELECT
                    ST_LineMerge(ST_Collect(ST_Segmentize(ST_Simplify(geometry, 24), 200))) AS geometry,
                    name,
                    type,
                    MIN(osm_id) AS osm_id
                FROM
                    osm_waterways
                WHERE
                    name <> '' AND
                    geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
                    {w}
                GROUP BY
                    name,
                    type
            )
            SELECT
                name,
                type,
                geometry
            FROM
                merged
            ORDER BY
                type <> 'river',
                osm_id
        ");

        client.query(&sql, &ctx.bbox_query_params(Some(2048.0)).as_params())
    })?;

    let mut options = TextOnLineOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            letter_spacing: 2.0,
            ..FontAndLayoutOptions::default()
        },
        color: colors::WATER_LABEL,
        halo_color: colors::WATER_LABEL_HALO,
        ..TextOnLineOptions::default()
    };

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        let typ = row.get_string("type")?;

        options.distribution = Distribution::Align {
            align: Align::Center,
            repeat: Repeat::Spaced(if typ == "river" { 400.0 } else { 300.0 }),
        };

        let name = replace(row.get_string("name")?, &REPLACEMENTS);

        walk_geometry_line_strings(&geom, &mut |geom| {
            let _drawn = draw_text_on_line(ctx.context, geom, &name, Some(collision), &options)?;

            cairo::Result::Ok(())
        })?;
    }

    Ok(())
}
