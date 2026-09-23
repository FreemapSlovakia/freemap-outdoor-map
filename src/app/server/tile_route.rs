use crate::{
    app::{
        server::app_state::{AppState, TileRouteState},
        tile_coord::TileCoord,
        tile_processor::{cached_tile_path, read_attribution},
    },
    render::{
        ATTRIBUTION_HEADER, Attribution, ImageFormat, RenderRequest, TileCoverageRelation,
        tile_touches_coverage,
    },
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, Response, StatusCode, header, response::Builder},
};
use geo::Rect;
use httpdate::parse_http_date;
use image::{ColorType, codecs::jpeg::JpegEncoder};
use std::{
    io::{self, Read},
    os::unix::fs::MetadataExt,
    sync::LazyLock,
    time::{Duration, SystemTime},
};
use tokio::task;

const TILE_CACHE_CONTROL: &str = "no-cache";

static GRAY_TILE_JPEG: LazyLock<Vec<u8>> = LazyLock::new(|| {
    const TILE_SIZE: usize = 256;
    const RED: u8 = 209;
    const GREEN: u8 = 204;
    const BLUE: u8 = 199;

    let mut pixels = vec![0; TILE_SIZE * TILE_SIZE * 3];

    for px in pixels.chunks_exact_mut(3) {
        px[0] = RED;
        px[1] = GREEN;
        px[2] = BLUE;
    }

    let mut encoded = Vec::new();

    JpegEncoder::new(&mut encoded)
        .encode(
            &pixels,
            TILE_SIZE as u32,
            TILE_SIZE as u32,
            ColorType::Rgb8.into(),
        )
        .expect("encode gray tile jpeg");

    encoded
});

#[derive(serde::Deserialize)]
pub struct QueryParams {
    rerender: Option<bool>,
}

pub async fn get(
    State(tile_route_state): State<TileRouteState>,
    Path((zoom, x, y_with_suffix)): Path<(u8, u32, String)>,
    Query(QueryParams { rerender }): Query<QueryParams>,
    headers: HeaderMap,
) -> Response<Body> {
    let state = tile_route_state.app_state;
    let variant_index = tile_route_state.variant_index;

    let Some((y, scale, ext)) = parse_y_suffix(&y_with_suffix) else {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("body should be built");
    };

    serve_tile(
        &state,
        variant_index,
        TileCoord { zoom, x, y },
        scale,
        ext,
        rerender.unwrap_or_default(),
        headers,
    )
    .await
}

pub async fn serve_tile(
    state: &AppState,
    variant_index: usize,
    coord: TileCoord,
    scale: f64,
    ext: Option<&str>,
    rerender: bool,
    headers: HeaderMap,
) -> Response<Body> {
    let Some(variant) = state.tile_variants.get(variant_index) else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("tile variant not found"))
            .expect("body should be built");
    };

    if coord.zoom < state.min_zoom || coord.zoom > state.max_zoom {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("body should be built");
    }

    if !state
        .allowed_scales
        .iter()
        .any(|allowed| (*allowed - scale).abs() < f64::EPSILON)
    {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("body should be built");
    }

    let ext = ext.unwrap_or("jpeg");

    if ext != "jpg" && ext != "jpeg" {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("body should be built");
    }

    let bbox = tile_bounds_to_epsg3857(coord.x, coord.y, coord.zoom, 256);

    if let Some(ref coverage_geometry) = variant.coverage_geometry {
        let meters_per_pixel = bbox.width() / 256.0;
        if tile_touches_coverage(coverage_geometry, bbox, meters_per_pixel)
            == TileCoverageRelation::Outside
        {
            // Outside coverage nothing is drawn, so the codes are known to be none —
            // which is not the same as the unknown of a tile cached without them.
            return annotate(
                Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "image/jpeg")
                    .header("Cache-Control", TILE_CACHE_CONTROL),
                TileSource::OutsideCoverage,
                Some(&Attribution::default()),
            )
            .body(Body::from(Bytes::from_static(GRAY_TILE_JPEG.as_slice())))
            .expect("body should be built");
        }
    }

    let file_path = if let Some(ref tile_cache_base_path) = variant.tile_cache_base_path {
        let file_path = cached_tile_path(tile_cache_base_path, coord, scale);

        enum ModifiedOrFresh {
            Modified(Vec<u8>, Option<SystemTime>, Option<Attribution>),
            Fresh(SystemTime, Option<Attribution>),
        }

        if rerender {
            // nothing
        } else if state.serve_cached {
            let if_modified_since = headers
                .get(header::IF_MODIFIED_SINCE)
                .and_then(|ims| parse_http_date(ims.to_str().ok()?).ok());

            // One blocking hop for the open, stat, xattr and read together.
            let result = task::spawn_blocking({
                let file_path = file_path.clone();

                move || -> io::Result<_> {
                    let mut f = std::fs::File::open(&file_path)?;

                    let metadata = f.metadata()?;

                    let mtime = metadata.modified().ok();

                    let attribution = read_attribution(&f);

                    if let Some(ims_time) = if_modified_since
                        && let Some(mtime) = mtime
                        && whole_seconds(mtime) <= ims_time
                    {
                        return Ok(ModifiedOrFresh::Fresh(mtime, attribution));
                    }

                    let mut buf = Vec::with_capacity(metadata.size() as usize);

                    f.read_to_end(&mut buf)?;

                    Ok(ModifiedOrFresh::Modified(buf, mtime, attribution))
                }
            })
            .await
            .unwrap_or_else(|err| Err(io::Error::other(err)));

            match result {
                Ok(ModifiedOrFresh::Modified(data, modified, attribution)) => {
                    let mut builder = annotate(
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "image/jpeg")
                            .header("Cache-Control", TILE_CACHE_CONTROL),
                        TileSource::Cache,
                        attribution.as_ref(),
                    );

                    if let Some(modified) = modified {
                        builder =
                            builder.header("Last-Modified", httpdate::fmt_http_date(modified));
                    }

                    return builder.body(Body::from(data)).expect("cached body");
                }
                Ok(ModifiedOrFresh::Fresh(date, attribution)) => {
                    return annotate(
                        Response::builder()
                            .status(StatusCode::NOT_MODIFIED)
                            .header("Cache-Control", TILE_CACHE_CONTROL)
                            .header("Last-Modified", httpdate::fmt_http_date(date)),
                        TileSource::Cache,
                        attribution.as_ref(),
                    )
                    .body(Body::empty())
                    .expect("empty body");
                }
                Err(err) => {
                    if err.kind() != std::io::ErrorKind::NotFound {
                        eprintln!("Read tile {coord}@{scale} failed: {err}");
                    }
                }
            }
        }

        Some(file_path)
    } else {
        None
    };

    let render_started_at = SystemTime::now();

    let render_request = RenderRequest::new(
        bbox,
        coord.zoom,
        scale,
        ImageFormat::Jpeg,
        variant.render.clone(),
        variant.coverage_geometry.clone(),
    );

    // println!("{coord}");

    let rendered = match state.render_worker_pool.render(render_request).await {
        Ok(rendered) => rendered,
        Err(err) => {
            eprintln!("Render tile {coord}@{scale} failed: {err}");

            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("render error"))
                .expect("body should be built");
        }
    };

    if file_path.is_some()
        && let Some(tile_worker) = state.tile_worker.as_ref()
        && let Err(err) = tile_worker
            .save_tile(
                rendered.bytes.clone(),
                rendered.attribution.clone(),
                coord,
                scale,
                render_started_at,
                variant_index,
            )
            .await
    {
        eprintln!("Enqueue tile {coord}@{scale} save failed: {err}");
    }

    annotate(
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "image/jpeg")
            .header("Cache-Control", TILE_CACHE_CONTROL)
            .header("Last-Modified", httpdate::fmt_http_date(render_started_at)),
        TileSource::Render,
        Some(&rendered.attribution),
    )
    .body(Body::from(rendered.bytes))
    .expect("body should be built")
}

/// A file mtime rounded down to what `Last-Modified` can express.
///
/// The tile's mtime is set from `SystemTime::now()` and keeps its nanoseconds,
/// but the header is whole seconds — so comparing the two directly makes a client
/// echoing back our own `Last-Modified` look stale by a fraction of a second, and
/// every revalidation ships the whole tile instead of a `304`.
fn whole_seconds(time: SystemTime) -> SystemTime {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(time, |since| {
            SystemTime::UNIX_EPOCH + Duration::from_secs(since.as_secs())
        })
}

/// Where the response body came from.
#[derive(Clone, Copy)]
enum TileSource {
    Cache,
    Render,
    /// The placeholder for a tile outside the variant's coverage: nothing is drawn,
    /// so it is neither cached nor rendered.
    OutsideCoverage,
}

impl TileSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Render => "render",
            Self::OutsideCoverage => "outside-coverage",
        }
    }
}

/// Annotates the response for the client fetching it: `X-Attribution` carries the
/// tile's dataset codes — absent rather than empty when they are unknown — and
/// `Server-Timing`'s `src` where the body came from. Each has its own cross-origin
/// gate (`Access-Control-Expose-Headers`, `Timing-Allow-Origin`); without it the
/// value reads as `null` or empty with no error anywhere.
fn annotate(builder: Builder, source: TileSource, attribution: Option<&Attribution>) -> Builder {
    let builder = builder
        .header("Server-Timing", format!("src;desc=\"{}\"", source.as_str()))
        .header("Timing-Allow-Origin", "*")
        .header("Access-Control-Expose-Headers", ATTRIBUTION_HEADER);

    match attribution {
        Some(attribution) => builder.header(ATTRIBUTION_HEADER, attribution.encode()),
        None => builder,
    }
}

fn parse_y_suffix(input: &str) -> Option<(u32, f64, Option<&str>)> {
    let mut y_part = input;
    let mut scale = 1.0;
    let mut ext = None;

    if let Some((left, right)) = input.split_once('@') {
        y_part = left;

        let (scale_str, rest) = right.split_once('x')?;

        scale = scale_str.parse::<f64>().ok()?;

        if let Some(after_dot) = rest.strip_prefix('.') {
            if after_dot.is_empty() {
                return None;
            }

            ext = Some(after_dot);
        } else if !rest.is_empty() {
            return None;
        }
    } else if let Some((left, right)) = input.split_once('.') {
        y_part = left;

        if right.is_empty() {
            return None;
        }

        ext = Some(right);
    }

    let y = y_part.parse::<u32>().ok()?;

    Some((y, scale, ext))
}

pub fn tile_bounds_to_epsg3857(x: u32, y: u32, zoom: u8, tile_size: u32) -> Rect<f64> {
    const HALF_CIRCUMFERENCE: f64 = std::f64::consts::PI * 6_378_137.0;

    let total_pixels = tile_size as f64 * (zoom as f64).exp2();
    let pixel_size = (2.0 * HALF_CIRCUMFERENCE) / total_pixels;

    let min_x = (x as f64 * tile_size as f64).mul_add(pixel_size, -HALF_CIRCUMFERENCE);
    let max_y = (y as f64 * tile_size as f64).mul_add(-pixel_size, HALF_CIRCUMFERENCE);

    let max_x = (tile_size as f64).mul_add(pixel_size, min_x);
    let min_y = (tile_size as f64).mul_add(-pixel_size, max_y);

    Rect::new((min_x, min_y), (max_x, max_y))
}

#[cfg(test)]
mod tests {
    use super::{TileSource, annotate, parse_y_suffix};
    use crate::render::Attribution;
    use axum::http::Response;

    fn header(attribution: Option<&Attribution>, name: &str) -> Option<String> {
        annotate(Response::builder(), TileSource::Cache, attribution)
            .body(())
            .expect("response")
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("ascii header").to_owned())
    }

    #[test]
    fn the_source_metric_stands_without_the_codes() {
        let mut attribution = Attribution::default();
        attribution.add_osm();

        for attribution in [Some(&attribution), None] {
            assert_eq!(
                header(attribution, "Server-Timing").as_deref(),
                Some("src;desc=\"cache\"")
            );

            // Cross-origin the page sees an empty `serverTiming` and no error
            // without this, so it goes out either way.
            assert_eq!(
                header(attribution, "Timing-Allow-Origin").as_deref(),
                Some("*")
            );
        }
    }

    #[test]
    fn the_attribution_header_keeps_known_empty_apart_from_unknown() {
        let mut attribution = Attribution::default();
        attribution.add_osm();
        attribution.add_shading("sk");
        attribution.add_contours("sk");

        assert_eq!(
            header(Some(&attribution), "X-Attribution").as_deref(),
            Some("csk,o,ssk")
        );

        // The gray tile credits nothing: present and empty, not absent.
        assert_eq!(
            header(Some(&Attribution::default()), "X-Attribution").as_deref(),
            Some("")
        );

        // Unknown is absent, which a client reads as "widen the credit".
        assert_eq!(header(None, "X-Attribution"), None);

        // A cross-origin `fetch` sees `null` without this, so it goes out either way.
        for attribution in [Some(&attribution), None] {
            assert_eq!(
                header(attribution, "Access-Control-Expose-Headers").as_deref(),
                Some("X-Attribution")
            );
        }
    }

    #[test]
    fn the_y_suffix_carries_the_scale_and_extension() {
        assert_eq!(parse_y_suffix("91000"), Some((91_000, 1.0, None)));
        assert_eq!(
            parse_y_suffix("91000.jpeg"),
            Some((91_000, 1.0, Some("jpeg")))
        );
        assert_eq!(parse_y_suffix("91000@2x"), Some((91_000, 2.0, None)));
        assert_eq!(
            parse_y_suffix("91000@2x.jpg"),
            Some((91_000, 2.0, Some("jpg")))
        );
        // Fractional scales are how the export-sized tiles are asked for.
        assert_eq!(parse_y_suffix("91000@1.5x"), Some((91_000, 1.5, None)));

        // A trailing dot names no extension, and anything after the scale that is
        // not one is not silently ignored.
        assert_eq!(parse_y_suffix("91000."), None);
        assert_eq!(parse_y_suffix("91000@2x."), None);
        assert_eq!(parse_y_suffix("91000@2xjunk"), None);
        assert_eq!(parse_y_suffix("91000@2"), None);
        assert_eq!(parse_y_suffix("91000@xx"), None);
        assert_eq!(parse_y_suffix("nine"), None);
        assert_eq!(parse_y_suffix(""), None);
    }

    #[test]
    fn a_client_echoing_our_last_modified_counts_as_fresh() {
        use super::whole_seconds;
        use std::time::{Duration, SystemTime};

        let mtime = SystemTime::UNIX_EPOCH
            + Duration::from_secs(1_757_000_000)
            + Duration::from_millis(734);
        // What `Last-Modified` said, and so what comes back in If-Modified-Since.
        let sent = SystemTime::UNIX_EPOCH + Duration::from_secs(1_757_000_000);

        assert!(mtime > sent, "the raw mtime looks newer than the header");
        assert!(whole_seconds(mtime) <= sent, "…but the tile is unchanged");

        // A genuinely older client copy is still stale.
        let older = SystemTime::UNIX_EPOCH + Duration::from_secs(1_756_999_999);
        assert!(whole_seconds(mtime) > older);
    }
}
