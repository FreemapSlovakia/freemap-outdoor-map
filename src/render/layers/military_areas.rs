use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{hatch::hatch_geometry, path_geom::path_geometry},
    FeatureError,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("military_areas::render");

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("military_areas", || {
        let sql = "
            SELECT
                geometry
            FROM
                osm_landcovers
            WHERE
                type = 'military'
                AND geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
                AND area / POWER(4, 19 - $6) > 10
        ";

        client.query(
            sql,
            &ctx.bbox_query_params(Some(10.0))
                .push(zoom as i32)
                .as_params(),
        )
    })?;

    let context = ctx.context;

    context.push_group();

    context.push_group();

    let tile_projector = &ctx.tile_projector;

    let geometries: Vec<_> = rows
        .iter()
        .map(|row| {
            let geom = row.get_geometry()?;
            Ok((geom.project_to_tile(tile_projector), geom))
        })
        .collect::<Result<Vec<_>, FeatureError>>()?;

    let context = context;

    // hatching
    for (projected, unprojected) in &geometries {
        context.push_group();

        path_geometry(context, projected);

        context.clip();

        context.set_source_color(colors::MILITARY);
        context.set_dash(&[], 0.0);
        context.set_line_width(1.5);

        hatch_geometry(context, unprojected, tile_projector, zoom, 10.0, -45.0)?;

        context.stroke()?;

        context.pop_group_to_source()?;
        context.paint()?;
    }

    context.pop_group_to_source()?;
    context.paint_with_alpha(if ctx.zoom < 14 { 0.5 / 0.8 } else { 0.2 / 0.8 })?;

    // border

    for (projected, _) in &geometries {
        context.set_source_color(colors::MILITARY);
        context.set_dash(&[25.0, 7.0], 0.0);
        context.set_line_width(3.0);
        path_geometry(context, projected);
        context.stroke()?;
    }

    context.pop_group_to_source()?;

    context.paint_with_alpha(0.8)?;

    Ok(())
}
