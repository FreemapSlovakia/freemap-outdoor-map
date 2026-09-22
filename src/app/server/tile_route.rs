use crate::{
    app::{
        server::app_state::{AppState, TileRouteState},
        tile_coord::TileCoord,
        tile_processor::cached_tile_path,
    },
    render::{
        Attribution, ImageFormat, JPEG_COM_HEAD_LEN, RenderRequest, TileCoverageRelation,
        parse_jpeg_com, tile_touches_coverage,
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
    os::unix::fs::MetadataExt,
    sync::LazyLock,
    time::{Duration, SystemTime},
};
use tokio::{
    fs,
    io::{self, AsyncReadExt},
};

/// `no-transform` because an image-rewriting intermediary would re-encode the tile
/// and drop the `COM` segment it carries its attribution in.
const TILE_CACHE_CONTROL: &str = "no-cache, no-transform";

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
            // which is not the same as the unknown of a tile with no `COM`.
            return with_timing(
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
            Modified(Vec<u8>, Option<SystemTime>),
            Fresh(SystemTime, Option<Attribution>),
        }

        if rerender {
            // nothing
        } else if state.serve_cached {
            let result: Result<_, io::Error> = async {
                let mut f = fs::OpenOptions::new().read(true).open(&file_path).await?;

                let metadata = f.metadata().await?;

                let mtime = metadata.modified().ok();

                if let Some(ims) = headers.get(header::IF_MODIFIED_SINCE)
                    && let Ok(ims_time) = parse_http_date(ims.to_str().unwrap_or(""))
                    && let Some(mtime) = mtime
                    && whole_seconds(mtime) <= ims_time
                {
                    // `Cache-Control: no-cache` makes every revisited tile revalidate,
                    // so this is the common path for a returning viewport and has to
                    // carry the codes too — one page-cached 1 KiB read for them.
                    return Ok(ModifiedOrFresh::Fresh(mtime, read_com(&mut f).await));
                }

                let mut buf = Vec::with_capacity(metadata.size() as usize);

                f.read_to_end(&mut buf).await?;

                Ok(ModifiedOrFresh::Modified(buf, mtime))
            }
            .await;

            match result {
                Ok(ModifiedOrFresh::Modified(data, modified)) => {
                    let attribution = parse_jpeg_com(&data).map(Attribution::decode);

                    let mut builder = with_timing(
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
                    return with_timing(
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
                coord,
                scale,
                render_started_at,
                variant_index,
            )
            .await
    {
        eprintln!("Enqueue tile {coord}@{scale} save failed: {err}");
    }

    with_timing(
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

/// The codes a cached tile carries, from its head bytes alone — the `COM` segment
/// sits among the first few, so the JPEG is never decoded and never fully read.
/// `None` when the tile has no segment, which is how one cached before tiles
/// carried their attribution goes out without an `attr` metric instead of
/// breaking.
async fn read_com(file: &mut fs::File) -> Option<Attribution> {
    let mut head = [0u8; JPEG_COM_HEAD_LEN];
    let mut read = 0;

    while read < head.len() {
        match file.read(&mut head[read..]).await {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => return None,
        }
    }

    parse_jpeg_com(&head[..read]).map(Attribution::decode)
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

/// Annotates the response with what the page displaying it needs to know about it.
///
/// `Server-Timing` is the one response header JavaScript can read off an `<img>`,
/// through `PerformanceResourceTiming.serverTiming` — so a client credits exactly
/// what is painted without fetching tile bytes or taking over the tile lifecycle.
/// `Timing-Allow-Origin` is what makes that work cross-origin: without it the
/// browser hands the page an empty `serverTiming` and reports nothing anywhere, so
/// it goes on every tile response.
///
/// Two metrics, comma-separated as the grammar wants — which is why the code list
/// inside `attr`'s `desc` is not:
///
/// - `src` — where the body came from, for anyone watching cache behaviour.
/// - `attr` — the tile's dataset codes, present exactly when they are known. A tile
///   cached before tiles carried attribution has none, and gets no `attr` rather
///   than an empty one that would read as "nothing to credit".
fn with_timing(
    builder: Builder,
    source: TileSource,
    attribution: Option<&Attribution>,
) -> Builder {
    let metrics = attribution.map_or_else(
        || format!("src;desc=\"{}\"", source.as_str()),
        |attribution| {
            format!(
                "src;desc=\"{}\", attr;desc=\"{}\"",
                source.as_str(),
                attribution.encode_spaced()
            )
        },
    );

    builder
        .header("Server-Timing", metrics)
        .header("Timing-Allow-Origin", "*")
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
    use super::{TileSource, parse_y_suffix, with_timing};
    use crate::render::Attribution;
    use axum::http::Response;

    fn header(attribution: Option<&Attribution>, name: &str) -> Option<String> {
        with_timing(Response::builder(), TileSource::Cache, attribution)
            .body(())
            .expect("response")
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("ascii header").to_owned())
    }

    #[test]
    fn the_metrics_separate_on_commas_and_the_code_list_does_not() {
        let mut attribution = Attribution::default();
        attribution.add_osm();
        attribution.add_shading("sk");
        attribution.add_contours("sk");

        // A comma inside `desc` would split the code list off into metrics of its own.
        assert_eq!(
            header(Some(&attribution), "Server-Timing").as_deref(),
            Some("src;desc=\"cache\", attr;desc=\"csk o ssk\"")
        );

        // Known to credit nothing (outside coverage) is an `attr` with an empty list…
        assert_eq!(
            header(Some(&Attribution::default()), "Server-Timing").as_deref(),
            Some("src;desc=\"cache\", attr;desc=\"\"")
        );

        // … while not knowing is no `attr` at all, and `src` still stands.
        assert_eq!(
            header(None, "Server-Timing").as_deref(),
            Some("src;desc=\"cache\"")
        );

        // Cross-origin the page sees an empty `serverTiming` and no error without
        // this, so it goes out either way.
        assert_eq!(
            header(Some(&attribution), "Timing-Allow-Origin").as_deref(),
            Some("*")
        );
        assert_eq!(header(None, "Timing-Allow-Origin").as_deref(), Some("*"));
    }

    #[test]
    fn the_y_suffix_carries_the_scale_and_extension() {
        assert_eq!(parse_y_suffix("91000"), Some((91_000, 1.0, None)));
        assert_eq!(parse_y_suffix("91000.jpeg"), Some((91_000, 1.0, Some("jpeg"))));
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
