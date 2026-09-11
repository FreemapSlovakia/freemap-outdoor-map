//! Guards the hand-written zoom ranges on the legend items against the renderer.
//!
//! The ranges in `legend::{default, feature_lines, landcovers, roads}` restate, in data, what
//! `layers::pipeline` and the layer render fns express as control flow, so that
//! `legend_metadata(Some(zoom))` can list only what the map draws at that zoom. This test
//! renders each item for real and fails when the two disagree — change a `zoom >= N` gate in
//! the renderer and you will hear about it here.

use super::{
    LegendItem, LegendItemData, LegendMode, MAX_LEGEND_ZOOM, legend_items, render_request,
    set_mapping_path_for_test,
};
use crate::render::{layers::Shading, renderer, set_fonts_path, svg_repo::SvgRepo};
use deadpool_postgres::{Config, Pool, Runtime};
use tokio::runtime::Handle;

/// Legend renders never touch the database, but the pipeline still wants a pool — this one is
/// never connected to.
fn unused_pool() -> Pool {
    let mut cfg = Config::new();

    cfg.url = Some("postgres://localhost/does-not-exist".to_owned());

    cfg.create_pool(Some(Runtime::Tokio1), tokio_postgres::NoTls)
        .expect("build db pool")
}

fn render(
    data: LegendItemData,
    zoom: u8,
    svg_repo: &mut SvgRepo,
    pool: &Pool,
    handle: &Handle,
) -> Vec<u8> {
    let request = render_request(data, zoom, 1.0, LegendMode::Normal);

    renderer::render(
        &request,
        Shading {
            hierarchy: None,
            contour_countries: None,
            feature_line_mask_countries: None,
            datasets: None,
        },
        None,
        pool.clone(),
        handle.clone(),
        svg_repo,
    )
    .expect("legend item rendered")
}

/// Whether anything of the item itself ends up on the tile — its background landcover is
/// rendered on its own and subtracted, since that draws at every zoom.
fn is_drawn(
    item: &LegendItem<'_>,
    rendered: &[u8],
    zoom: u8,
    svg_repo: &mut SvgRepo,
    pool: &Pool,
    handle: &Handle,
) -> bool {
    rendered != render(item.background.clone(), zoom, svg_repo, pool, handle)
}

#[test]
fn zoom_ranges_match_what_the_renderer_draws() {
    set_mapping_path_for_test();
    set_fonts_path("fonts".into());

    let mut svg_repo = SvgRepo::new("images");

    let pool = unused_pool();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let handle = rt.handle().clone();

    let mut mismatches = Vec::new();

    for zoom in 0..=MAX_LEGEND_ZOOM {
        for item in legend_items(Some(zoom), false) {
            // Every item at every zoom, if only to prove the render does not fail — a legend
            // feature missing a property the renderer reads at some zoom errors out here.
            let rendered = render(item.render_data(), zoom, &mut svg_repo, &pool, &handle);

            // The ranges are plain thresholds, so only their edges are compared against the
            // background; doing that everywhere would double an already slow test.
            let start = *item.zooms.start();
            let end = *item.zooms.end();

            let below_start = zoom + 1 == start;

            if zoom != start && zoom != end && !below_start && zoom != end + 1 {
                continue;
            }

            if below_start && !item.probe_lower_edge {
                continue;
            }

            let expected = item.zooms.contains(&zoom);

            if is_drawn(item, &rendered, zoom, &mut svg_repo, &pool, &handle) != expected {
                mismatches.push(format!(
                    "{} declares {start}..={end} but is {} drawn at zoom {zoom}",
                    item.meta.id,
                    if expected { "not" } else { "still" },
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "legend zoom ranges disagree with the renderer:\n{}",
        mismatches.join("\n")
    );
}
