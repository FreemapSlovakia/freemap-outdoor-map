use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{hatch::hatch_geometry, path_geom::path_geometry},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;
use postgres::{Client, Row};

pub fn query(ctx: &Ctx, client: &mut Client) -> Result<Vec<Row>, postgres::Error> {
    let table_suffix = match ctx.zoom {
        ..=9 => "_gen0",
        10..=11 => "_gen1",
        12.. => "",
    };

    #[cfg_attr(any(), rustfmt::skip)]
        let sql = format!("
            SELECT
                geometry,
                COALESCE(intermittent OR seasonal, false) AS tmp
            FROM
                osm_waterareas{table_suffix}
            WHERE
                geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)
        ");

    client.query(&sql, &ctx.bbox_query_params(None).as_params())
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("water_areas::render");

    let zoom = ctx.zoom;

    let tile_projector = &ctx.tile_projector;

    context.save()?;

    for row in rows {
        let geom = row.get_geometry()?;

        let projected = geom.project_to_tile(tile_projector);

        let tmp: bool = row.get_bool("tmp")?;

        if tmp {
            context.push_group();

            path_geometry(context, &projected);

            context.clip();

            context.set_source_color(colors::WATER);
            context.paint()?;

            context.set_source_color_a(colors::WHITE, 0.75);
            context.set_dash(&[], 0.0);
            context.set_line_width(2.0);

            hatch_geometry(context, &geom, tile_projector, zoom, 4.0, 0.0)?;

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
