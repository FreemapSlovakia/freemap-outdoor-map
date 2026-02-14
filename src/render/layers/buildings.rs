use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("buildings::render");

    let rows = ctx.legend_features("buildings", || {
        let sql = "
            SELECT
                type,
                geometry
            FROM
                osm_buildings
            WHERE
                geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)
        ";

        client.query(sql, &ctx.bbox_query_params(None).as_params())
    })?;

    let context = ctx.context;

    context.save()?;

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        path_geometry(context, &geom);

        let typ = row.get_string("type")?;

        if typ.starts_with("disused:") || typ == "disused" {
            context.push_group();

            context.set_source_color(colors::BUILDING); // any
            context.fill_preserve()?;

            context.push_group();
            context.set_source_color_a(colors::BUILDING, 0.66);
            context.fill_preserve()?;
            context.set_dash(&[3.0, 3.0], 0.0);
            context.set_line_width(2.0);
            context.set_source_color(colors::BUILDING);
            context.stroke()?;
            context.pop_group_to_source()?;
            context.set_operator(cairo::Operator::DestIn);
            context.paint()?;

            context.pop_group_to_source()?;
            context.paint()?;
        } else if typ.starts_with("abandoned:") || typ == "abandoned" {
            context.push_group();

            context.set_source_color(colors::BUILDING); // any
            context.fill_preserve()?;

            context.push_group();
            context.set_source_color_a(colors::BUILDING, 0.33);
            context.fill_preserve()?;
            context.set_dash(&[3.0, 3.0], 0.0);
            context.set_line_width(2.0);
            context.set_source_color(colors::BUILDING);
            context.stroke()?;
            context.pop_group_to_source()?;
            context.set_operator(cairo::Operator::DestIn);
            context.paint()?;

            context.pop_group_to_source()?;
            context.paint()?;
        } else if typ.starts_with("ruins:") || typ == "ruins" {
            context.push_group();

            context.set_source_color(colors::BUILDING); // any
            context.fill_preserve()?;

            context.push_group();
            context.set_dash(&[3.0, 3.0], 0.0);
            context.set_line_width(2.0);
            context.set_source_color(colors::BUILDING);
            context.stroke()?;
            context.pop_group_to_source()?;
            context.set_operator(cairo::Operator::DestIn);
            context.paint()?;

            context.pop_group_to_source()?;
            context.paint()?;
        } else {
            context.set_source_color(colors::BUILDING);
            context.fill()?;
        }
    }

    context.restore()?;

    Ok(())
}
