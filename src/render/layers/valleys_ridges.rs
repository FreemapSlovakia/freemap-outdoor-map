use crate::render::{
    Feature,
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    regex_replacer::{Replacement, replace},
};
use cairo::Context;
use geo::ChaikinSmoothing;
use cosmic_text::Style;
use regex::Regex;
use std::sync::LazyLock;

static REPLACEMENTS: LazyLock<Vec<Replacement>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"^Dolink?a\b *").expect("regex"), "Dol. "),
        (Regex::new(r"^dolink?a\b *").expect("regex"), "dol. "),
        (Regex::new(r" *\b[Dd]olink?a$").expect("regex"), " dol."),
    ]
});

pub async fn query_valleys(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let dir = if ctx.zoom > 14 { "ASC" } else { "DESC" };

    #[cfg_attr(any(), rustfmt::skip)]
    let sql = format!("
        SELECT
            geometry,
            name,
            LEAST(1.2, ST_Length(geometry) / 5000) AS offset_factor
        FROM
            osm_feature_lines
        WHERE
            type = 'valley' AND
            name <> '' AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            ST_Length(geometry) {dir}
    ");

    client.query(&sql, &ctx.bbox_query_params(Some(512.0)).as_params()).await
}

pub async fn query_ridges(ctx: &Ctx, client: &tokio_postgres::Client) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            geometry, name, 0::double precision AS offset_factor
        FROM
            osm_feature_lines
        WHERE
            type = 'ridge' AND
            name <> '' AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            ST_Length(geometry) DESC
    ";

    client.query(sql, &ctx.bbox_query_params(Some(512.0)).as_params()).await
}

const fn size_factors(zoom: u8) -> &'static [f64] {
    match zoom {
        ..=13 => &[1.0],
        14 => &[1.0, 0.75],
        _ => &[1.0, 0.75, 0.5],
    }
}

fn render_rows(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    letter_spacing: f64,
    size: f64,
    off: f64,
) -> LayerRenderResult {
    let collision = &mut Collision::new(Some(context));

    for row in rows {
        let name = replace(row.get_string("name")?, &REPLACEMENTS);

        let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

        let offset_factor = row.get_f64("offset_factor")?;

        let geom = geom.chaikin_smoothing(3);

        // The label does not grow relatively shorter as you zoom in, so a valley too short
        // for its name at full size stays that way at every zoom: it needs a smaller face,
        // not a closer look.
        for (attempt, factor) in size_factors(ctx.zoom).iter().enumerate() {
            let size = size * factor;

            let mut options = TextOnLineOptions {
                flo: FontAndLayoutOptions {
                    style: Style::Italic,
                    // `letter_spacing` is in pixels, so it does not follow the smaller face.
                    // Tracking it out again would only widen a label the full size already
                    // failed to fit untracked.
                    letter_spacing: if attempt == 0 { letter_spacing } else { 0.0 },
                    size,
                    ..Default::default()
                },
                color: colors::TRAM,
                halo_opacity: 0.9,
                distribution: Distribution::Align {
                    align: Align::Center,
                    repeat: Repeat::Spaced(200.0),
                },
                offset: offset_factor.mul_add(off, size / 2.0),
                ..Default::default()
            };

            let mut drawn = false;

            #[allow(clippy::while_float)]
            while options.flo.letter_spacing >= 0.0 {
                drawn = draw_text_on_line(context, &geom, &name, Some(collision), &options)?;

                if drawn {
                    break;
                }

                options.flo.letter_spacing = (options.flo.letter_spacing + 1.0).mul_add(0.8, -2.0);
            }

            if drawn {
                break;
            }
        }
    }

    Ok(())
}

pub fn render_valleys(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("valleys_ridges::render_valleys");

    let zoom_coef = 2.5f64.powf(ctx.zoom as f64 - 12.0);
    let letter_spacing = 15.0 + zoom_coef;
    let size = 10.0 + zoom_coef;
    let off = 1.5f64.mul_add(zoom_coef, 6.0);

    render_rows(ctx, context, rows, letter_spacing, size, off)
}

pub fn render_ridges(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("valleys_ridges::render_ridges");

    let zoom_coef = 2.5f64.powf(ctx.zoom as f64 - 12.0);
    let letter_spacing = 15.0 + zoom_coef;
    let size = 10.0 + zoom_coef;
    let off = 1.5f64.mul_add(zoom_coef, 6.0);

    render_rows(ctx, context, rows, letter_spacing, size, off)
}
