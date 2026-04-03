use crate::render::{
    Feature,
    ctx::Ctx,
    draw::{markers_on_path::draw_markers_on_path, path_geom::path_line_string},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use cairo::Context;
use postgres::Client;

pub fn query_points(ctx: &Ctx, client: &mut Client) -> Result<Vec<postgres::Row>, postgres::Error> {
    let sql = "
        SELECT
            geometry
        FROM
            osm_fixmes
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            osm_id
    ";

    client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())
}

pub fn query_lines(ctx: &Ctx, client: &mut Client) -> Result<Vec<postgres::Row>, postgres::Error> {
    let sql = "
        SELECT * FROM (
            SELECT geometry, fixme FROM osm_feature_lines
            UNION
            SELECT geometry, fixme FROM osm_roads
        ) foo
        WHERE
            fixme <> '' AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)";

    client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())
}

pub fn render_points(
    ctx: &Ctx,
    context: &Context,
    points: Vec<Feature>,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("fixmes::render_points");

    let surface = svg_repo.get("fixme")?;

    let rect = surface.extents().expect("surface extents");

    let hw = rect.width() / 2.0;

    let hh = rect.height() / 2.0;

    for row in points {
        let point = row.get_point()?.project_to_tile(&ctx.tile_projector).0;

        context.set_source_surface(surface, (point.x - hw).round(), (point.y - hh).round())?;

        context.paint()?;
    }

    Ok(())
}

pub fn render_lines(
    ctx: &Ctx,
    context: &Context,
    lines: Vec<Feature>,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("fixmes::render_lines");

    let surface = svg_repo.get("fixme")?;

    let rect = surface.extents().expect("surface extents");

    let hw = rect.width() / 2.0;

    let hh = rect.height() / 2.0;

    for row in lines {
        let line_string = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

        path_line_string(context, &line_string);

        let path = context.copy_path_flat()?;

        context.new_path();

        draw_markers_on_path(&path, 75.0, 150.0, &|x, y, _angle| {
            context.set_source_surface(surface, (x - hw).round(), (y - hh).round())?;
            context.paint()?;
            Ok(())
        })?;
    }

    Ok(())
}
