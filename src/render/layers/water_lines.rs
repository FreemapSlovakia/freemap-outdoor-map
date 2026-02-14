use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{markers_on_path::draw_markers_on_path, smooth_line::path_smooth_bezier_spline},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("water_lines::render");

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("water_lines", || {
        let geom_query = match zoom {
            12 => "ST_Segmentize(ST_Simplify(geometry, 24), 200) AS geometry",
            13 => "ST_Segmentize(ST_Simplify(geometry, 12), 200) AS geometry",
            14 => "ST_Segmentize(ST_Simplify(geometry, 6), 200) AS geometry",
            _ => "geometry",
        };

        let table = match zoom {
            ..=9 => "osm_waterways_gen0",
            10..=11 => "osm_waterways_gen1",
            _ => "osm_waterways",
        };

        #[cfg_attr(any(), rustfmt::skip)]
        let sql = format!("
            SELECT
                {geom_query},
                type,
                seasonal OR intermittent AS tmp,
                tunnel
            FROM
                {table}
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ");

        client.query(&sql, &ctx.bbox_query_params(Some(8.0)).as_params())
    })?;

    // TODO lazy
    let arrow = svg_repo.get("waterway-arrow")?;

    let (dx, dy) = {
        let rect = arrow.extents().expect("surface extents");

        (-rect.width() / 2.0, -rect.height() / 2.0)
    };

    let context = ctx.context;

    context.save()?;

    for pass in 0..=1 {
        let glow = pass == 0;

        for row in &rows {
            let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

            let typ = row.get_string("type")?;

            let tmp = row.get_bool("tmp")?;
            let tunnel = row.get_bool("tunnel")?;

            context.set_dash(if tmp { &[6.0, 3.0] } else { &[] }, 0.0);

            let (width, smooth) = match (typ, zoom) {
                ("river" | "canal", ..=8) => (1.5f64.powf(zoom as f64 - 8.0), 0.0),
                ("river" | "canal", 9) => (1.5, 0.0),
                ("river" | "canal", 10..=11) => (2.2, 0.0),
                ("river" | "canal", 12..) => (2.2, 0.5),
                (
                    "canoe_pass" | "ditch" | "drain" | "fish_pass" | "rapids" | "ressurised"
                    | "stream" | "tidal_channel",
                    12..,
                ) => (if zoom == 12 { 1.0 } else { 1.2 }, 0.5),

                _ => continue,
            };

            if glow {
                if zoom >= 12 {
                    context.set_source_color(colors::WATER);

                    context.set_source_rgba(1.0, 1.0, 1.0, if tunnel { 0.8 } else { 0.5 });

                    context.set_line_width(if matches!(typ, "river" | "canal") {
                        3.4
                    } else if zoom == 12 {
                        2.0
                    } else {
                        2.4
                    });

                    path_smooth_bezier_spline(context, &geom, smooth);

                    context.stroke()?;
                }
            } else {
                context.set_source_color_a(colors::WATER, if tunnel { 0.33 } else { 1.0 });

                context.set_line_width(width);

                path_smooth_bezier_spline(context, &geom, smooth);

                let path = context.copy_path_flat()?;

                context.stroke()?;

                draw_markers_on_path(&path, 150.0, 300.0, &|x, y, angle| -> cairo::Result<()> {
                    context.save()?;
                    context.translate(x, y);
                    context.rotate(angle);
                    context.set_source_surface(arrow, dx, dy)?;
                    context.paint()?;
                    context.restore()?;

                    Ok(())
                })?;
            }
        }
    }

    context.restore()?;

    Ok(())
}
