use crate::render::{
    Feature, ctx::Ctx, draw::line_pattern::draw_line_pattern,
    layer_render_error::LayerRenderResult, projectable::TileProjectable, svg_repo::SvgRepo,
};
use cairo::Context;
use postgres::{Client, Row};

pub fn query(ctx: &Ctx, client: &mut Client) -> Result<Vec<Row>, postgres::Error> {
    let sql = "
        SELECT
            geometry
        FROM
            osm_roads
        WHERE
            embankment = 1 AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
    ";

    client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("embankments::render");

    for row in rows {
        let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

        draw_line_pattern(context, ctx.size, &geom, 0.8, svg_repo.get("embankment")?)?;
    }

    Ok(())
}
