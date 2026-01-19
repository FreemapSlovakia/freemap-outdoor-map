use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_line_string},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("aeroways::render");

    let (way_width, dash_width, dash_array) = match ctx.zoom {
        11 => (3.0, 0.5, &[3.0, 3.0]),
        12..=13 => (5.0, 1.0, &[4.0, 4.0]),
        14.. => (8.0, 1.0, &[6.0, 6.0]),
        _ => panic!("unsupported zoom"),
    };

    let sql = concat!(
        "SELECT geometry, type FROM osm_aeroways ",
        "WHERE geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)"
    );

    let rows = client.query(sql, &ctx.bbox_query_params(Some(12.0)).as_params())?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        path_line_string(
            context,
            &geometry_line_string(&row).project_to_tile(&ctx.tile_projector),
        );

        context.set_source_color(colors::AEROWAY);
        context.set_dash(&[], 0.0);
        context.set_line_width(way_width);
        context.stroke_preserve()?;

        context.set_source_rgb(1.0, 1.0, 1.0);
        context.set_line_width(dash_width);
        context.set_dash(dash_array, 0.0);

        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}
