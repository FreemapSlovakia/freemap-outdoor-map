use postgres::Client;

use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_line_string},
};

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("barrierways::render");

    let sql = "
        SELECT
            geometry,
            type
        FROM
            osm_barrierways
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            osm_id
    ";

    let rows = client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())?;

    for row in rows {
        let context = ctx.context;

        context.save()?;

        match row.get("type") {
            "city_wall" => {
                context.set_dash(&[], 0.0);
                context.set_source_color(colors::BUILDING);
                context.set_line_width(2.0);
            }
            "hedge" => {
                context.set_source_color(colors::PITCH);
                context.set_line_width(ctx.zoom as f64 - 14.0);
                context.set_dash(&[0.01, ctx.zoom as f64 - 14.0], 0.0);
                context.set_line_join(cairo::LineJoin::Round);
                context.set_line_cap(cairo::LineCap::Round);
            }
            _ => {
                context.set_dash(&[2.0, 1.0], 0.0);
                context.set_line_width(1.0);
                context.set_source_color(colors::BARRIERWAY);
            }
        }

        path_line_string(
            context,
            &geometry_line_string(&row).project_to_tile(&ctx.tile_projector),
        );

        context.stroke()?;

        context.restore()?;
    }

    Ok(())
}
