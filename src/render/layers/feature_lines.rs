use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{line_pattern::draw_line_pattern_scaled, path_geom::path_line_string},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("feature_lines::render");

    let rows = ctx.legend_features("feature_lines", || {
        let sql = "
            SELECT
                geometry,
                type
            FROM
                osm_feature_lines
            WHERE
                type IN ('weir', 'dam', 'tree_row') AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ";

        client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())
    })?;

    let context = ctx.context;

    for row in rows {
        let geom = row.line_string()?.project_to_tile(&ctx.tile_projector);

        context.save()?;

        let zoom = ctx.zoom;

        match row.get_string("type")? {
            "weir" => {
                if zoom >= 16 {
                    context.set_dash(&[9.0, 3.0], 0.0);
                    context.set_source_color(colors::DAM_LINE);
                    context.set_line_width(3.0);
                    path_line_string(context, &geom);
                    context.stroke()?;
                }
            }
            "dam" => {
                if zoom >= 16 {
                    context.set_source_color(colors::DAM_LINE);
                    context.set_line_width(3.0);
                    path_line_string(context, &geom);
                    context.stroke()?;
                }
            }
            "tree_row" => {
                draw_line_pattern_scaled(
                    ctx.context,
                    ctx.size,
                    &geom,
                    0.8,
                    (2.0 + (zoom as f64 - 15.0).exp2()) / 4.5,
                    svg_repo.get("tree2")?,
                )?;
            }
            _ => panic!("unexpected type"),
        }

        context.restore()?;
    }

    Ok(())
}
