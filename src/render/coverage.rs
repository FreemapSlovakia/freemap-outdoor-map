use geo::{Contains, Geometry, Intersects, Rect};

pub(crate) const MAX_EDGE_FADE_RADIUS_M: f64 = 5_000.0;
pub(crate) const EDGE_FADE_CUTOFF_SIGMA: f64 = 3.0;
pub(crate) const MAX_EDGE_FADE_SIGMA_PX: f64 = 10.0;

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum TileCoverageRelation {
    Inside,
    Crosses,
    Outside,
}

pub(crate) fn tile_touches_coverage(
    coverage: &Geometry,
    bbox: Rect<f64>,
    meters_per_pixel: f64,
) -> TileCoverageRelation {
    let min = bbox.min();
    let max = bbox.max();
    let edge_fade_cutoff_m = edge_fade_cutoff_m(meters_per_pixel);

    let buffered_bbox = Rect::new(
        (min.x - edge_fade_cutoff_m, min.y - edge_fade_cutoff_m),
        (max.x + edge_fade_cutoff_m, max.y + edge_fade_cutoff_m),
    );

    if coverage.contains(&buffered_bbox) {
        TileCoverageRelation::Inside
    } else if coverage.intersects(&buffered_bbox) {
        TileCoverageRelation::Crosses
    } else {
        TileCoverageRelation::Outside
    }
}

#[inline]
pub(crate) fn edge_fade_sigma_px(meters_per_pixel: f64) -> f64 {
    (MAX_EDGE_FADE_RADIUS_M / meters_per_pixel / EDGE_FADE_CUTOFF_SIGMA).min(MAX_EDGE_FADE_SIGMA_PX)
}

#[inline]
pub(crate) fn edge_fade_cutoff_px(meters_per_pixel: f64) -> f64 {
    edge_fade_cutoff_m(meters_per_pixel) / meters_per_pixel
}

#[inline]
fn edge_fade_cutoff_m(meters_per_pixel: f64) -> f64 {
    let cutoff_from_data_m = MAX_EDGE_FADE_RADIUS_M;
    let cutoff_from_sigma_m =
        edge_fade_sigma_px(meters_per_pixel) * EDGE_FADE_CUTOFF_SIGMA * meters_per_pixel;

    cutoff_from_data_m.min(cutoff_from_sigma_m)
}
