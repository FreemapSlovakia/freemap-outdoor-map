use crate::render::{
    collision::Collision,
    colors::{self},
    ctx::Ctx,
    draw::{
        path_geom::walk_geometry_line_strings,
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
};

use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, collision: &mut Collision) -> LayerRenderResult {
    let _span = tracy_client::span!("highway_names::render");

    let sql = "
        WITH merged AS (
            SELECT
                name,
                ST_LineMerge(ST_Collect(geometry)) AS geometry,
                type,
                z_order,
                MIN(osm_id) AS osm_id
            FROM
                osm_roads
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                name <> '' AND
                NOT (class = 'railway' AND type = 'abandoned')
                GROUP BY z_order, name, type
        )
        SELECT
            name,
            geometry,
            type
        FROM
            merged
        ORDER BY
            z_order DESC,
            osm_id
    ";

    let rows = client.query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params())?;

    let options = TextOnLineOptions {
        distribution: Distribution::Align {
            align: Align::Center,
            repeat: Repeat::Spaced(200.0),
        },
        color: colors::TRACK,
        ..TextOnLineOptions::default()
    };

    for row in rows {
        let Some(geom) = geometry_geometry(&row) else {
            continue;
        };

        let geom = geom.project_to_tile(&ctx.tile_projector);

        let name: &str = row.get("name");

        walk_geometry_line_strings(&geom, &mut |geom| {
            let _drawn = draw_text_on_line(ctx.context, geom, name, Some(collision), &options)?;

            cairo::Result::Ok(())
        })?;
    }

    Ok(())
}
