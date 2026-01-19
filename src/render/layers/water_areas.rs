use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{hatch::hatch_geometry, path_geom::path_geometry},
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("water_areas::render");

    let table_suffix = match ctx.zoom {
        ..=9 => "_gen0",
        10..=11 => "_gen1",
        12.. => "",
    };

    let rows = client.query(
        &format!(
            "SELECT
                type, geometry, COALESCE(intermittent OR seasonal, false) AS tmp
            FROM osm_waterareas{table_suffix}
            WHERE geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)"
        ),
        &ctx.bbox_query_params(None).as_params(),
    )?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        let Some(geom) = geometry_geometry(&row) else {
            continue;
        };

        let projected = geom.project_to_tile(&ctx.tile_projector);

        let tmp: bool = row.get("tmp");

        if tmp {
            context.push_group();

            path_geometry(context, &projected);

            context.clip();

            context.set_source_color(colors::WATER);
            context.paint()?;

            context.set_source_color_a(colors::WHITE, 0.75);
            context.set_dash(&[], 0.0);
            context.set_line_width(2.0);

            hatch_geometry(ctx, &geom, 4.0, 0.0)?;

            context.stroke()?;

            context.pop_group_to_source()?;
            context.paint()?;
        } else {
            context.set_source_color(colors::WATER);

            path_geometry(context, &projected);

            context.fill()?;
        }
    }

    context.restore()?;

    Ok(())
}
