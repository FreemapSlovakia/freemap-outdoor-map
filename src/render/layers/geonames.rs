use crate::render::{
    colors::{self},
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        text_on_line::{Distribution, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use pangocairo::pango::Style;
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("geonames::render");

    let rows = ctx.legend_features("geonames", || {
        let sql = "
            SELECT
                name,
                geometry FROM geonames_smooth
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                ogc_fid
        ";

        client.query(sql, &ctx.bbox_query_params(Some(20.0)).as_params())
    })?;

    let options = TextOnLineOptions {
        flo: FontAndLayoutOptions {
            size: 8.0 + 1.9f64.powf(ctx.zoom as f64 - 6.0),
            style: Style::Italic,
            ..Default::default()
        },
        distribution: Distribution::Justify {
            min_spacing: Some(0.0),
        },
        halo_opacity: 1.0,
        color: colors::TRAM,
        halo_width: 2.0,
        ..TextOnLineOptions::default()
    };

    let context = ctx.context;

    context.push_group();

    for row in rows {
        let name = row.get_string("name")?;

        let geom = row.line_string()?.project_to_tile(&ctx.tile_projector);

        let mut options = options;

        for _ in 0..3 {
            if draw_text_on_line(context, &geom, name, None, &options)? {
                break;
            }

            options.flo.size *= 0.9;
            options.halo_width *= 0.9;
        }
    }

    context.pop_group_to_source()?;

    context.paint_with_alpha(0.8 - 1.5f64.powf(ctx.zoom as f64 - 9.0) / 5.0)?;

    Ok(())
}
