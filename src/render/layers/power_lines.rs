use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_line_string, geometry_point},
};
use postgres::Client;

pub fn render_lines(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("power_lines::render_lines");

    let sql = &format!(
        "SELECT geometry, type FROM osm_feature_lines WHERE {} AND geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)",
        if ctx.zoom < 14 {
            "type = 'line'"
        } else {
            "type IN ('line', 'minor_line')"
        }
    );

    let rows = client.query(sql, &ctx.bbox_query_params(None).as_params())?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        context.set_source_color_a(
            if row.get::<_, &str>("type") == "line" {
                colors::POWER_LINE
            } else {
                colors::POWER_LINE_MINOR
            },
            0.5,
        );

        context.set_dash(&[], 0.0);
        context.set_line_width(1.0);

        path_line_string(
            context,
            &geometry_line_string(&row).project_to_tile(&ctx.tile_projector),
        );

        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}

pub fn render_towers_poles(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("power_lines::render_towers_poles");

    let sql = format!(
        "SELECT geometry, type
        FROM osm_features
        WHERE type IN ('power_tower'{}) AND geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)",
        if ctx.zoom < 15 { "" } else { ", 'pylon', 'pole'" }
    );

    let rows = client.query(&sql, &ctx.bbox_query_params(Some(1024.0)).as_params())?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        context.set_source_color(if row.get::<_, &str>("type") == "pole" {
            colors::POWER_LINE_MINOR
        } else {
            colors::POWER_LINE
        });

        let p = geometry_point(&row).project_to_tile(&ctx.tile_projector);

        // TODO align by scale
        context.rectangle((p.x() - 1.5).round(), (p.y() - 1.5).round(), 3.0, 3.0);

        context.fill()?;
    }

    context.restore()?;

    Ok(())
}
