use crate::render::{
    Feature,
    collision::Collision,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        path_geom::{path_geometry, walk_geometry_line_strings},
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

pub const MIN_ZOOM: u8 = 11;

pub const NAMES_MIN_ZOOM: u8 = 13;

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    // Merged per name so a route split into several ways gets labelled along its
    // whole length; unnamed ways stay apart.
    let sql = "
        SELECT
            ST_LineMerge(ST_Collect(geometry)) AS geometry,
            name
        FROM
            osm_ferries
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        GROUP BY
            name,
            CASE WHEN name = '' THEN osm_id END
        ORDER BY
            MIN(osm_id)
    ";

    client
        .query(sql, &ctx.bbox_query_params(Some(512.0)).as_params())
        .await
}

pub fn render(ctx: &Ctx, context: &Context, rows: &[Feature]) -> LayerRenderResult {
    let _span = tracy_client::span!("ferries::render");

    context.save()?;

    context.set_source_color(colors::FERRY);
    context.set_line_width(if ctx.zoom < 13 { 1.0 } else { 1.5 });
    context.set_dash(&[6.0, 4.0], 0.0);

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        path_geometry(context, &geom);
    }

    context.stroke()?;

    context.restore()?;

    Ok(())
}

pub fn render_names(
    ctx: &Ctx,
    context: &Context,
    rows: &[Feature],
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("ferries::render_names");

    let options = TextOnLineOptions {
        distribution: Distribution::Align {
            align: Align::Center,
            repeat: Repeat::Spaced(400.0),
        },
        color: colors::FERRY,
        ..TextOnLineOptions::default()
    };

    for row in rows {
        let name = row.get_string("name")?;

        if name.is_empty() {
            continue;
        }

        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        walk_geometry_line_strings(&geom, &mut |geom| {
            draw_text_on_line(context, geom, name, Some(collision), &options)?;

            cairo::Result::Ok(())
        })?;
    }

    Ok(())
}
