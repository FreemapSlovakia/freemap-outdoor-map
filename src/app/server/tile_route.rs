use crate::{
    app::{server::app_state::AppState, tile_coord::TileCoord, tile_processor::cached_tile_path},
    render::{ImageFormat, RenderRequest, TileCoverageRelation, tile_touches_coverage},
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{Response, StatusCode},
};
use geo::Rect;
use image::{ColorType, codecs::jpeg::JpegEncoder};
use std::{sync::LazyLock, time::SystemTime};
use tokio::fs;

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

pub(crate) async fn get(
    State(state): State<AppState>,
    Path((zoom, x, y_with_suffix)): Path<(u8, u32, String)>,
) -> Response<Body> {
    let Some((y, scale, ext)) = parse_y_suffix(&y_with_suffix) else {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("body should be built");
    };

    serve_tile(&state, TileCoord { zoom, x, y }, scale, ext).await
}

pub(crate) async fn serve_tile(
    state: &AppState,
    coord: TileCoord,
    scale: f64,
    ext: Option<&str>,
) -> Response<Body> {
    if coord.zoom > state.max_zoom {
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

    if let Some(ref coverage_geometry) = state.coverage_geometry {
        let meters_per_pixel = bbox.width() / 256.0;
        if tile_touches_coverage(coverage_geometry, bbox, meters_per_pixel)
            == TileCoverageRelation::Outside
        {
            return Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "image/jpeg")
                .body(Body::from(Bytes::from_static(GRAY_TILE_JPEG.as_slice())))
                .expect("body should be built");
        }
    }

    let render_request = RenderRequest::new(
        bbox,
        coord.zoom,
        scale,
        ImageFormat::Jpeg,
        state.render.to_owned(),
    );

    let file_path = if let Some(ref tile_cache_base_path) = state.tile_cache_base_path {
        let file_path = cached_tile_path(tile_cache_base_path, coord, scale);

        if state.serve_cached {
            match fs::read(&file_path).await {
                Ok(data) => {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "image/jpeg")
                        .body(Body::from(data))
                        .expect("cached body");
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
            .save_tile(rendered.clone(), coord, scale, render_started_at)
            .await
    {
        eprintln!("Enqueue tile {coord}@{scale} save failed: {err}");
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/jpeg")
        .body(Body::from(rendered))
        .expect("body should be built")
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
