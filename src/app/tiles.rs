use crate::{
    app::{app_state::AppState, index_paths::index_file_path},
    render::{ImageFormat, RenderRequest, tile_bounds_to_epsg3857},
};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{Response, StatusCode},
};
use fs2::FileExt;
use geo::algorithm::intersects::Intersects;
use std::{io::Write, path::PathBuf};
use tokio::fs;

pub(crate) async fn tile_get(
    State(state): State<AppState>,
    Path((zoom, x, y_with_suffix)): Path<(u32, u32, String)>,
) -> Response<Body> {
    let Some((y, scale, ext)) = parse_y_suffix(&y_with_suffix) else {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("body should be built");
    };

    serve_tile(&state, zoom, x, y, scale, ext).await
}

pub(crate) async fn serve_tile(
    state: &AppState,
    zoom: u32,
    x: u32,
    y: u32,
    scale: f64,
    ext: Option<&str>,
) -> Response<Body> {
    if zoom > state.max_zoom {
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

    let bbox = tile_bounds_to_epsg3857(x, y, zoom, 256);

    if let Some(ref limits_geometry) = *state.limits_geometry {
        let tile_polygon = bbox.to_polygon();

        if !limits_geometry.intersects(&tile_polygon) {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .expect("body should be built");
        }
    }

    let tile_request = RenderRequest::new(bbox, zoom, vec![scale], ImageFormat::Jpeg);

    let file_path = if let Some(ref tile_base_path) = *state.tile_base_path {
        let file_path = tile_cache_path(tile_base_path, zoom, x, y, scale);

        match fs::read(&file_path).await {
            Ok(data) => {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "image/jpeg")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Body::from(data))
                    .expect("cached body");
            }
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("read tile failed: {err}");
                }
            }
        }

        append_index_entry(tile_base_path, state.index_zoom, zoom, x, y, scale);

        Some(file_path)
    } else {
        None
    };

    let rendered = match state.worker_pool.render(tile_request).await {
        Ok(rendered) => rendered,
        Err(err) => {
            eprintln!("render failed: {err}");

            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("render error"))
                .expect("body should be built");
        }
    };

    if let Some(tile) = rendered.into_iter().next() {
        if let Some(file_path) = file_path {
            if let Some(parent) = file_path.parent()
                && let Err(err) = fs::create_dir_all(parent).await
            {
                eprintln!("create tile dir failed: {err}");
            }

            if let Err(err) = fs::write(&file_path, &tile).await {
                eprintln!("write tile failed: {err}");
            }
        }

        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "image/jpeg")
            .header("Access-Control-Allow-Origin", "*")
            .body(Body::from(tile))
    } else {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("empty render result"))
    }
    .expect("body should be built")
}

fn tile_cache_path(base: &std::path::Path, zoom: u32, x: u32, y: u32, scale: f64) -> PathBuf {
    let mut path = base.to_owned();
    path.push(zoom.to_string());
    path.push(x.to_string());
    path.push(format!("{y}@{scale}.jpeg"));
    path
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

fn append_index_entry(
    base: &std::path::Path,
    index_zoom: u32,
    zoom: u32,
    x: u32,
    y: u32,
    scale: f64,
) {
    if zoom <= index_zoom {
        return;
    }

    let shift = zoom - index_zoom;

    let index_path = index_file_path(base, index_zoom, x >> shift, y >> shift);

    if let Some(parent) = index_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!("create index dir failed: {err}");
        return;
    }

    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&index_path)
    {
        Ok(file) => file,
        Err(err) => {
            eprintln!("open index file failed: {err}");
            return;
        }
    };

    if let Err(err) = file.lock_exclusive() {
        eprintln!("lock index file failed: {err}");
        return;
    }

    if let Err(err) = file.write_all(format!("{zoom}/{x}/{y}@{scale}\n").as_bytes()) {
        eprintln!("write index entry failed: {err}");
    }
}
