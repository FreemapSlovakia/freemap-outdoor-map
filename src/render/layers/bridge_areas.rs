use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, mask: bool) -> LayerRenderResult {
    let _span = tracy_client::span!("bridge_areas::render");

    let query = "
        SELECT
            geometry
        FROM
            osm_landcovers
        WHERE
            geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857) AND
            type = 'bridge'
    ";

    let rows = client.query(query, &ctx.bbox_query_params(None).as_params())?;

    let context = ctx.context;

    context.save()?;

    if mask {
        context.set_fill_rule(cairo::FillRule::EvenOdd);
    }

    for row in rows {
        let Some(geometry) =
            geometry_geometry(&row).map(|geom| geom.project_to_tile(&ctx.tile_projector))
        else {
            continue;
        };

        if mask {
            context.rectangle(0.0, 0.0, ctx.size.width as f64, ctx.size.height as f64);
            path_geometry(context, &geometry);
            context.clip();
        } else {
            path_geometry(context, &geometry);
            context.set_source_color(colors::INDUSTRIAL);
            context.fill_preserve()?;

            context.set_line_width(1.0);
            context.set_dash(&[], 0.0);
            context.set_source_color(colors::BUILDING);
            context.stroke()?;
        }
    }

    context.restore()?;

    Ok(())
}
