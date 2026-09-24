use crate::render::{
    Feature, FeatureError, RenderLayer,
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

/// How the grade is stored, which decides how it is read and filtered. Both
/// forms keep the filter a bare column predicate, so a partial index on it is
/// usable — a cast around the column is not, and the planner falls back to
/// scanning every road in the tile.
#[derive(Clone, Copy)]
pub enum Grade {
    /// An imposm `enumerate`: 0 untagged, then 1-based into `colors`.
    Enumerate,
    /// A scale the mapping's expression folded to the same 1-based index, but
    /// written as text because an imposm expression yields a string.
    FoldedText,
}

/// A way graded on an ordered scale, drawn as dots along the path in the grade's
/// colour.
///
/// Opaque on purpose. A translucent band reads as terrain rather than as a marked
/// route, but an overlay sits above every other layer, where a translucent mark
/// takes its colour from whatever happens to be beneath it.
pub struct GradedDots {
    /// The `osm_roads` column holding the grade.
    pub column: &'static str,
    pub kind: Grade,
    pub colors: &'static [Color],
}

impl GradedDots {
    /// SQL yielding the 1-based grade.
    fn grade_sql(&self) -> String {
        match self.kind {
            Grade::Enumerate => self.column.to_owned(),
            Grade::FoldedText => format!("{}::int", self.column),
        }
    }

    /// SQL selecting the graded ways, shaped to match the partial index in
    /// `sql/additional.sql`.
    fn filter_sql(&self) -> String {
        match self.kind {
            Grade::Enumerate => format!("{} > 0", self.column),
            Grade::FoldedText => format!("{} <> ''", self.column),
        }
    }
}

/// Below this the generalized road tables have dropped the ways that carry a
/// grade, so there is nothing left to read one from.
pub const MIN_ZOOM: u8 = 12;

pub const SAC_SCALE: GradedDots = GradedDots {
    column: "sac_scale",
    kind: Grade::Enumerate,
    colors: &colors::SAC_SCALE,
};

pub const SMOOTHNESS: GradedDots = GradedDots {
    column: "smoothness",
    kind: Grade::Enumerate,
    colors: &colors::SMOOTHNESS,
};

pub const PISTE_DIFFICULTY: GradedDots = GradedDots {
    column: "piste_difficulty",
    kind: Grade::Enumerate,
    colors: &colors::PISTE_DIFFICULTY,
};

pub const MTB_SCALE: GradedDots = GradedDots {
    column: "mtb_scale",
    kind: Grade::FoldedText,
    colors: &colors::MTB_SCALE,
};

pub const VIA_FERRATA_SCALE: GradedDots = GradedDots {
    column: "via_ferrata_scale",
    kind: Grade::FoldedText,
    colors: &colors::VIA_FERRATA_SCALE,
};

/// Every graded overlay: the layer that switches it on, the stage it registers
/// under, and how it is drawn. One table so a sixth cannot be half-added.
pub const ALL: [(RenderLayer, &str, &GradedDots); 5] = [
    (RenderLayer::SacScale, "sac_scale", &SAC_SCALE),
    (RenderLayer::Smoothness, "smoothness", &SMOOTHNESS),
    (RenderLayer::MtbScale, "mtb_scale", &MTB_SCALE),
    (RenderLayer::PisteDifficulty, "piste_difficulty", &PISTE_DIFFICULTY),
    (RenderLayer::ViaFerrataScale, "via_ferrata_scale", &VIA_FERRATA_SCALE),
];

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

    let grade = dots.grade_sql();
    let filter = dots.filter_sql();

    #[cfg_attr(any(), rustfmt::skip)]
    let sql = format!("
        SELECT
            geometry,
            {grade} AS grade
        FROM
            osm_roads
        WHERE
            {filter} AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            grade
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
    for run in geoms.chunk_by(|(a, _), (b, _)| a == b) {
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
