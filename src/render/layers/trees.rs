use crate::render::{
    Feature, ctx::Ctx, layer_render_error::LayerRenderResult, projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use cairo::Context;

pub async fn query(ctx: &Ctx, client: &tokio_postgres::Client) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            type,
            geometry
        FROM
            osm_pois
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            (
                (
                    type = 'tree' AND
                    (NOT (tags ? 'protected') OR tags->'protected' = 'no') AND
                    (NOT (tags ? 'denotation') OR tags->'denotation' <> 'natural_monument')
                )
                OR type = 'shrub'
            )
        ORDER BY
            type,
            st_x(geometry),
            osm_id
    ";

    client.query(sql, &ctx.bbox_query_params(Some(32.0)).as_params()).await
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("trees::render");

    for row in rows {
        let typ = row.get_string("type")?;

        let point = row.get_point()?.project_to_tile(&ctx.tile_projector);

        let scale =
            (2.0 + (ctx.zoom as f64 - 15.0).exp2()) * (if typ == "shrub" { 0.1 } else { 0.2 });

        let surface = svg_repo.get("tree2")?;

        let rect = surface.extents().expect("surface extents");

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
