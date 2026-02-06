use crate::render::{
    ctx::Ctx,
    draw::{markers_on_path::draw_markers_on_path, path_geom::path_line_string},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use postgres::Client;
use std::cell::Cell;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("road_access_restrictions::render");

    // TODO lazy

    let no_bicycle_icon = &svg_repo.get("no_bicycle")?.clone();

    let no_foot_icon = &svg_repo.get("no_foot")?.clone();

    let no_bicycle_rect = no_bicycle_icon.extents().expect("surface extents");

    let no_foot_rect = no_foot_icon.extents().expect("surface extents");

    let rows = ctx.legend_features("road_access_restrictions", || {
        let sql = "
            SELECT
                CASE
                    WHEN bicycle NOT IN ('', 'yes', 'designated', 'official', 'permissive')
                        OR bicycle = '' AND vehicle NOT IN ('', 'yes', 'designated', 'official', 'permissive')
                        OR bicycle = '' AND vehicle = '' AND access NOT IN ('', 'yes', 'designated', 'official', 'permissive')
                    THEN 1 ELSE 0 END AS no_bicycle,
                CASE
                    WHEN foot NOT IN ('', 'yes', 'designated', 'official', 'permissive')
                        OR foot = '' AND access NOT IN ('', 'yes', 'designated', 'official', 'permissive')
                    THEN 1
                ELSE 0
                END AS no_foot,
                geometry
            FROM
                osm_roads
            WHERE
                type NOT IN ('trunk', 'motorway', 'trunk_link', 'motorway_link') AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ";

        client.query(sql, &ctx.bbox_query_params(Some(32.0)).as_params())
    })?;

    let context = ctx.context;

    for row in rows {
        let geom = row.line_string()?.project_to_tile(&ctx.tile_projector);

        path_line_string(context, &geom);

        let path = context.copy_path_flat()?;

        context.new_path();

        let no_bicycle = row.get_i32("no_bicycle")? > 0;
        let no_foot = row.get_i32("no_foot")? > 0;

        if !no_bicycle && !no_foot {
            continue;
        }

        let i_cell = Cell::new(0);

        draw_markers_on_path(&path, 12.0, 24.0, &|x, y, angle| -> cairo::Result<()> {
            let i = i_cell.get();

            let (arrow, rect) = if no_bicycle && no_foot && i % 2 == 0 {
                (no_bicycle_icon, no_bicycle_rect)
            } else if no_foot {
                (no_foot_icon, no_foot_rect)
            } else {
                (no_bicycle_icon, no_bicycle_rect)
            };

            context.save()?;
            context.translate(x, y);
            context.rotate(angle);
            context.set_source_surface(arrow, -rect.width() / 2.0, -rect.height() / 2.0)?;
            context.paint_with_alpha(0.75)?;
            context.restore()?;

            i_cell.set(i + 1);

            Ok(())
        })?;
    }

    Ok(())
}
