use crate::render::{
    Feature, FeatureError,
    colors::{self, Color, ContextExt, WHITE},
    ctx::Ctx,
    draw::path_geom::path_line_string,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

/// Width of the white ring around each dot, per side.
const OUTLINE_PX: f64 = 0.7;

/// Gap between dot centres, as a multiple of the dot's own diameter.
const SPACING: f64 = 2.2;

/// A way graded on an ordered scale, drawn as dots along the path in the grade's
/// colour.
///
/// Opaque on purpose. A translucent band reads as terrain rather than as a marked
/// route, but an overlay sits above every other layer, where a translucent mark
/// takes its colour from whatever happens to be beneath it.
pub struct GradedDots {
    /// The `osm_roads` column holding the grade, as an imposm enumerate: 0 for
    /// untagged, then 1-based into `colors`.
    pub column: &'static str,
    pub colors: &'static [Color],
}

/// Below this the generalized road tables have dropped the ways that carry a
/// grade, so there is nothing left to read one from.
pub const MIN_ZOOM: u8 = 12;

pub const SAC_SCALE: GradedDots = GradedDots {
    column: "sac_scale",
    colors: &colors::SAC_SCALE,
};

pub const SMOOTHNESS: GradedDots = GradedDots {
    column: "smoothness",
    colors: &colors::SMOOTHNESS,
};

fn dot_diameter(zoom: u8) -> f64 {
    (1.4f64.powf(f64::from(zoom) - 14.0) * 5.0).min(14.0)
}

pub async fn query(
    dots: &GradedDots,
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    // Half a dot, plus its ring, bleeds in from either side.
    let buffer_px = dot_diameter(ctx.zoom).mul_add(0.5, OUTLINE_PX + 1.0);

    let column = dots.column;

    #[cfg_attr(any(), rustfmt::skip)]
    let sql = format!("
        SELECT
            geometry,
            {column} AS grade
        FROM
            osm_roads
        WHERE
            {column} > 0 AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            {column}
    ");

    client.query(&sql, &ctx.bbox_query_params(Some(buffer_px)).as_params()).await
}

pub fn render(
    dots: &GradedDots,
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
) -> LayerRenderResult {
    let _span = tracy_client::span!("graded_dots::render");

    let diameter = dot_diameter(ctx.zoom);

    // A zero-length dash under a round cap is a dot; the gap sets their spacing.
    let dashes = [0.001, diameter * SPACING];

    let geoms = rows
        .iter()
        .map(|row| {
            Ok((
                row.get_i32("grade")?,
                row.get_line_string()?.project_to_tile(&ctx.tile_projector),
            ))
        })
        .collect::<Result<Vec<_>, FeatureError>>()?;

    context.save()?;

    context.set_line_cap(cairo::LineCap::Round);
    context.set_dash(&dashes, 0.0);

    // Every ring first, so a dot is never punched out by its neighbour's ring.
    context.set_source_color(WHITE);
    context.set_line_width(OUTLINE_PX.mul_add(2.0, diameter));

    for (_, geom) in &geoms {
        path_line_string(context, geom);
    }

    context.stroke()?;

    context.set_line_width(diameter);

    // Ordered by grade, so equal grades arrive in runs: one path and one stroke
    // each, rather than one per way. The dots are opaque and of one width, so
    // overlapping strokes of a run composite identically either way. Drawing the
    // runs in order is also what puts the harder grade on top where paths meet.
    let mut runs = geoms.chunk_by(|(a, _), (b, _)| a == b);

    for run in &mut runs {
        let Ok(index) = usize::try_from(run[0].0 - 1) else {
            continue;
        };

        let Some(color) = dots.colors.get(index) else {
            continue;
        };

        context.set_source_color(*color);

        for (_, geom) in run {
            path_line_string(context, geom);
        }

        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}
