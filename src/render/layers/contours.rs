use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        smooth_line::path_smooth_bezier_spline,
        text_on_line::{
            Align, Distribution, Repeat, TextOnLineOptions, Upright, draw_text_on_line,
        },
    },
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_line_string},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, country: Option<&str>) -> LayerRenderResult {
    let _span = tracy_client::span!("contours::render");

    let context = ctx.context;
    let zoom = ctx.zoom;

    if zoom < 12 {
        return Ok(());
    }

    let simplify_factor: f64 = match zoom {
        ..=12 => 2000.0,
        13 => 1000.0,
        14 => 200.0,
        15 => 50.0,
        _ => 0.0,
    };

    context.save()?;

    // TODO measure performance impact of simplification, if it makes something faster
    let width_case = match zoom {
        12 => "CASE WHEN height_m % 50 = 0 THEN 0.2 ELSE 0.0 END",
        13 | 14 => {
            "CASE
                WHEN height_m % 100 = 0 THEN 0.4
                WHEN height_m % 20 = 0 THEN 0.2
                ELSE 0.0
            END"
        }
        _ => {
            "CASE
                WHEN height_m % 100 = 0 THEN 0.6
                WHEN height_m % 10 = 0 THEN 0.3
                WHEN height_m % 50 = 0 AND height_m % 100 <> 0 THEN 0.0
                ELSE 0.0
            END"
        }
    };

    let sql = format!(
        "WITH contours AS (
            SELECT
                ST_SimplifyVW(wkb_geometry, $6) AS geometry,
                height_m,
                ({width_case})::double precision AS width
            FROM {}
            WHERE
                wkb_geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        )
        SELECT geometry, height_m, width FROM contours WHERE width > 0",
        &(if let Some(country) = country {
            format!("contour_{country}_split")
        } else {
            "cont_dmr_split".to_string()
        })
    );

    let mut params = ctx.bbox_query_params(Some(8.0));

    params.push(simplify_factor);

    let query_params = params.as_params();

    let rows = client.query(&sql, &query_params)?;

    for row in rows {
        let height: i16 = row.get("height_m");

        let width: f64 = row.get("width");

        let labels = match zoom {
            13..=14 => height % 100 == 0,
            15.. => height % 50 == 0,
            _ => false,
        };

        let geom = geometry_line_string(&row).project_to_tile(&ctx.tile_projector);

        context.set_dash(&[], 0.0);

        context.set_line_width(width);

        context.set_source_color(colors::CONTOUR);

        path_smooth_bezier_spline(context, &geom, 1.0);

        context.stroke()?;

        if labels {
            let _drawn = draw_text_on_line(
                context,
                &geom,
                &format!("{}", height),
                None,
                &TextOnLineOptions {
                    flo: FontAndLayoutOptions::default(),
                    upright: Upright::Left,
                    color: colors::CONTOUR,
                    distribution: Distribution::Align {
                        align: Align::Center,
                        repeat: Repeat::Spaced(200.0),
                    },
                    ..TextOnLineOptions::default()
                },
            )?;
        }
    }

    context.restore()?;

    Ok(())
}
