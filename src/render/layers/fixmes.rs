use crate::render::{
    SvgRepo,
    ctx::Ctx,
    draw::{markers_on_path::draw_markers_on_path, path_geom::path_line_string},
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_line_string, geometry_point},
};
use geo::Coord;
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("fixmes::render");

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

    let rows = client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())?;

    let surface = svg_repo.get("fixme")?;

    let rect = surface.extents().expect("surface extents");

    let hw = rect.width() / 2.0;

    let hh = rect.height() / 2.0;

    let context = ctx.context;

    let paint = |point: &Coord| -> cairo::Result<()> {
        context.set_source_surface(surface, (point.x - hw).round(), (point.y - hh).round())?;

        context.paint()?;

        Ok(())
    };

    for row in rows {
        paint(&geometry_point(&row).project_to_tile(&ctx.tile_projector).0)?;
    }

    let sql = "
        SELECT * FROM (
            SELECT geometry, fixme FROM osm_feature_lines
            UNION
            SELECT geometry, fixme FROM osm_roads
        ) foo
        WHERE
            fixme <> '' AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)";

    let rows = client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())?;

    for row in rows.into_iter() {
        let line_string = geometry_line_string(&row).project_to_tile(&ctx.tile_projector);

        path_line_string(context, &line_string);

        let path = context.copy_path_flat()?;

        context.new_path();

        draw_markers_on_path(&path, 75.0, 150.0, &|x, y, _angle| paint(&Coord { x, y }))?;
    }

    Ok(())
}
