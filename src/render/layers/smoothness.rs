use crate::render::{
    Feature, colors,
    ctx::Ctx,
    draw::graded_dots::{self, GradedDots},
    layer_render_error::LayerRenderResult,
};
use cairo::Context;

/// As for `sac_scale`: the generalized road tables keep only major roads and
/// railways, so below this there is no row left to read `smoothness` from.
pub const MIN_ZOOM: u8 = 12;

const DOTS: GradedDots = GradedDots {
    min_zoom: MIN_ZOOM,
    column: "smoothness",
    colors: &colors::SMOOTHNESS,
};

pub async fn query(ctx: &Ctx, client: &tokio_postgres::Client) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    graded_dots::query(&DOTS, ctx, client).await
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("smoothness::render");

    graded_dots::render(&DOTS, ctx, context, rows)
}
