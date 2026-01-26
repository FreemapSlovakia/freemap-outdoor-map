use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        offset_line::offset_line_string,
        text_on_line::{Distribution, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    layers::borders,
    projectable::{TileProjectable, geometry_line_string},
};
use postgres::Client;
use std::f64;

pub fn render(ctx: &Ctx, client: &mut Client) -> LayerRenderResult {
    let _span = tracy_client::span!("country_names::render");

    let context = ctx.context;

    let rect = ctx.bbox.project_to_tile(&ctx.tile_projector);

    context.save()?;
    context.rectangle(rect.min().x, rect.min().y, rect.width(), rect.height());
    context.set_source_color_a(colors::WHITE, 0.33);
    context.fill()?;
    context.restore()?;

    borders::render(ctx, client)?;

    let sr = 1.5f64.powf(ctx.zoom as f64 - 6.0).max(0.66);

    let options_upper = TextOnLineOptions {
        flo: FontAndLayoutOptions {
            size: sr * 20.0,
            ..Default::default()
        },
        halo_width: 2.0,
        distribution: Distribution::Justify {
            min_spacing: Some(0.0),
        },
        concave_spacing_factor: 0.0,
        ..Default::default()
    };

    let options_lower = TextOnLineOptions {
        flo: FontAndLayoutOptions {
            size: sr * 16.0,
            ..Default::default()
        },
        halo_width: 2.0,
        color: colors::AREA_LABEL,
        distribution: Distribution::Justify {
            min_spacing: Some(0.0),
        },
        concave_spacing_factor: 0.0,
        ..Default::default()
    };

    let sql = r#"
        SELECT
            name,
            "name:en",
            geometry
        FROM
            country_names_smooth
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
    "#;

    let rows = client.query(sql, &ctx.bbox_query_params(Some(128.0)).as_params())?;

    for row in rows {
        let name: &str = row.get("name");

        let name_en: &str = row.get("name:en");

        let geom = geometry_line_string(&row).project_to_tile(&ctx.tile_projector);

        // context.save();
        // path_line_string(context, &geom);
        // context.set_source_color(colors::BLACK);
        // context.set_line_width(2.0);
        // context.stroke();
        // context.restore();

        context.push_group();

        for (name, mut offset, options) in [
            (name, sr * -14.0, &options_upper),
            (name_en, sr * 8.0, &options_lower),
        ] {
            let mut options = *options;

            // TODO offset_line_string produces bad results for `align: Align::Justify`
            // options.offset = offset;

            while options.flo.size > 10.0 {
                let geom = offset_line_string(&geom, offset);

                if draw_text_on_line(context, &geom, name, None, &options)? {
                    break;
                }

                options.flo.size *= 0.9;
                options.halo_width *= 0.9;
                offset *= 0.9;
            }
        }

        context.pop_group_to_source()?;
        context.paint_with_alpha(0.66)?;
    }

    Ok(())
}
