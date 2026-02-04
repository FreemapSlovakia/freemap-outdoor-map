use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{hatch::hatch_geometry, path_geom::path_geometry},
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("solar_power_plants::render");

    let zoom = ctx.zoom;

    let d = 4.0f64.max(1.33f64.powf(zoom as f64) / 20.0).round();

    let sql = "
        SELECT
            geometry FROM osm_power_generators
        WHERE
            source = 'solar' AND geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)
        ORDER BY osm_id
    ";

    let rows = client.query(sql, &ctx.bbox_query_params(None).as_params())?;

    let context = ctx.context;

    let tile_projector = &ctx.tile_projector;

    for row in rows {
        let Some(geom) = geometry_geometry(&row) else {
            continue;
        };

        context.push_group();

        let projected = geom.project_to_tile(tile_projector);

        path_geometry(context, &projected);

        let path = context.copy_path()?;

        context.save()?;

        context.clip();

        context.set_source_color(colors::SOLAR_BG);
        context.paint()?;

        context.set_source_color(colors::SOLAR_FG);
        context.set_dash(&[], 0.0);
        context.set_line_width(1.0);

        hatch_geometry(context, &geom, tile_projector, zoom, d, 0.0)?;
        hatch_geometry(context, &geom, tile_projector, zoom, d, 90.0)?;

        context.stroke()?;

        context.restore()?;

        context.new_path();
        context.append_path(&path);

        context.set_source_color(colors::SOLAR_PLANT_BORDER);
        context.set_dash(&[], 0.0);
        context.set_line_width(1.0);
        context.stroke()?;

        context.pop_group_to_source()?;
        context.paint()?;
    }

    Ok(())
}
