use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::{path_geometry, path_line_string_with_offset, walk_geometry_line_strings},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("special_parks::render");

    // TODO consired area
    // TODO maybe move to landcovers.rs

    let rows = ctx.legend_features("special_parks", || {
        let sql = "
            SELECT
                geometry
            FROM
                osm_landcovers
            WHERE
                type IN ('zoo', 'theme_park') AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ";

        client.query(sql, &ctx.bbox_query_params(Some(10.0)).as_params())
    })?;

    let context = ctx.context;

    context.push_group();

    context.set_line_join(cairo::LineJoin::Miter);
    context.set_line_cap(cairo::LineCap::Square);

    let wb = 14.0 - (150.0 / (ctx.zoom as f64));

    for row in rows {
        let geometry = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        context.set_source_color(colors::SPECIAL_PARK);
        context.set_dash(&[], 0.0);
        context.set_line_width((wb * 0.33).max(1.0));
        path_geometry(context, &geometry);
        context.stroke()?;

        context.set_line_width(wb);
        context.set_source_color_a(colors::SPECIAL_PARK, 0.5);
        walk_geometry_line_strings(&geometry, &mut |iter| {
            path_line_string_with_offset(context, iter, wb * 0.5);

            cairo::Result::Ok(())
        })?;
        context.stroke()?;
    }

    context.pop_group_to_source()?;

    context.paint_with_alpha(0.66)?;

    Ok(())
}
