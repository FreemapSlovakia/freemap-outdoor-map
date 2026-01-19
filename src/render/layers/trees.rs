use crate::render::{
    SvgRepo,
    ctx::Ctx,
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_point},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("trees::render");

    let sql = "
        SELECT type, geometry
        FROM osm_features
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            (
                type = 'tree' AND (NOT (tags ? 'protected') OR tags->'protected' = 'no') AND (NOT (tags ? 'denotation') OR tags->'denotation' <> 'natural_monument')
                OR type = 'shrub'
            )
        ORDER BY type, st_x(geometry)";

    let rows = client.query(sql, &ctx.bbox_query_params(Some(32.0)).as_params())?;

    for row in rows {
        let typ: &str = row.get("type");

        let point = geometry_point(&row).project_to_tile(&ctx.tile_projector);

        let scale =
            (2.0 + (ctx.zoom as f64 - 15.0).exp2()) * (if typ == "shrub" { 0.1 } else { 0.2 });

        let surface = svg_repo.get("tree2")?;

        let rect = surface.extents().expect("surface extents");

        let context = ctx.context;

        context.save()?;

        context.translate(
            point.x() - scale * rect.width() / 2.0,
            point.y() - scale * rect.height() / 2.0,
        );

        context.scale(scale, scale);

        context.set_source_surface(surface, 0.0, 0.0)?;

        context.paint()?;

        context.restore()?;
    }

    Ok(())
}
