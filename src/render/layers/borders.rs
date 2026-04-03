use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

pub async fn query(ctx: &Ctx, client: &tokio_postgres::Client) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            geometry
        FROM
            osm_country_members
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
    ";

    client.query(sql, &ctx.bbox_query_params(Some(10.0)).as_params()).await
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("borders::render");

    context.push_group();

    for row in rows {
        let geometry = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        context.set_dash(&[], 0.0);
        context.set_source_color(colors::ADMIN_BORDER);
        context.set_line_width(if ctx.zoom <= 10 {
            6.0f64.mul_add(1.4f64.powf(ctx.zoom as f64 - 11.0), 0.5)
        } else {
            6.0
        });
        context.set_line_cap(cairo::LineCap::Square);
        context.set_line_join(cairo::LineJoin::Round);
        path_geometry(context, &geometry);
        context.stroke()?;
    }

    context.pop_group_to_source()?;
    context.paint_with_alpha(0.5)?;

    Ok(())
}
