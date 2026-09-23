use crate::render::{
    Feature, FeatureError,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

/// The generalized road tables keep only major roads and railways, so below this
/// there is no row left to read `sac_scale` from.
pub const MIN_ZOOM: u8 = 12;

/// Width of the white ring around each dot, per side.
const OUTLINE_PX: f64 = 0.7;

/// Gap between dot centres, as a multiple of the dot's own diameter.
const SPACING: f64 = 2.2;

/// Dots stay legible over anything, so the layer draws fully opaque — an overlay
/// sits above every other layer, where a translucent mark would take its colour
/// from whatever happens to be beneath it.
fn dot_diameter(zoom: u8) -> f64 {
    (1.4f64.powf((zoom as f64).max(f64::from(MIN_ZOOM)) - 14.0) * 5.0).min(14.0)
}

pub async fn query(ctx: &Ctx, client: &tokio_postgres::Client) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    // Half a dot, plus its ring, bleeds in from either side.
    let buffer_px = dot_diameter(ctx.zoom).mul_add(0.5, OUTLINE_PX + 1.0);

    #[cfg_attr(any(), rustfmt::skip)]
    let sql = "
        SELECT
            geometry,
            sac_scale
        FROM
            osm_roads
        WHERE
            sac_scale > 0 AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            sac_scale
    ";

    client.query(sql, &ctx.bbox_query_params(Some(buffer_px)).as_params()).await
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("sac_scale::render");

    let diameter = dot_diameter(ctx.zoom);

    // A zero-length dash under a round cap is a dot; the gap sets their spacing.
    let dashes = [0.001, diameter * SPACING];

    let geoms = rows
        .iter()
        .map(|row| {
            Ok((
                row.get_i32("sac_scale")?,
                row.get_line_string()?.project_to_tile(&ctx.tile_projector),
            ))
        })
        .collect::<Result<Vec<_>, FeatureError>>()?;

    context.save()?;

    context.set_line_cap(cairo::LineCap::Round);
    context.set_dash(&dashes, 0.0);

    // Every ring first, so a dot is never punched out by its neighbour's ring.
    context.set_source_color(colors::WHITE);
    context.set_line_width(OUTLINE_PX.mul_add(2.0, diameter));

    for (_, geom) in &geoms {
        path_line_string(context, geom);
    }

    context.stroke()?;

    context.set_line_width(diameter);

    // The query orders by grade, so where two paths meet the harder one is on top.
    for (sac_scale, geom) in &geoms {
        let Ok(index) = usize::try_from(*sac_scale - 1) else {
            continue;
        };

        let Some(color) = colors::SAC_SCALE.get(index) else {
            continue;
        };

        context.set_source_color(*color);

        path_line_string(context, geom);

        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}
