use crate::{
    app::server::app_state::AppState,
    render::{Layers, LegendMode, legend_metadata, legend_render_request},
};
use axum::{
    Json,
    response::IntoResponse,
    body::Body,
    extract::{Path, Query, State},
    http::{Response, StatusCode},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LegendMetadataQuery {
    /// List only the items the map draws at this zoom; defaults to listing all of them.
    zoom: Option<u8>,
    /// List only what this tile route draws, by its URL path (`/`, `/o/sac`, …);
    /// defaults to the whole catalogue.
    variant: Option<String>,
}

#[derive(Deserialize)]
pub struct LegendQuery {
    scale: Option<f64>,
    mode: Option<LegendMode>,
    /// Draw the sample as this tile route draws it — an overlay's samples carry
    /// no ground, and it has no item for anything it does not draw.
    variant: Option<String>,
    /// Render the item as it appears at this zoom; defaults to the item's preferred zoom.
    zoom: Option<u8>,
}

pub async fn get_metadata(
    State(state): State<AppState>,
    Query(LegendMetadataQuery { zoom, variant }): Query<LegendMetadataQuery>,
) -> Response<Body> {
    let layers = match variant_layers(&state, variant.as_deref()) {
        Ok(layers) => layers,
        Err(response) => return *response,
    };

    Json(legend_metadata(zoom, layers.as_ref())).into_response()
}

/// The selection a `?variant=` names, or the 404 to answer with.
fn variant_layers(
    state: &AppState,
    variant: Option<&str>,
) -> Result<Option<Layers>, Box<Response<Body>>> {
    let Some(path) = variant else {
        return Ok(None);
    };

    state
        .variant_by_path(path)
        .map(|variant| Some(variant.layers.clone()))
        .ok_or_else(|| {
            Box::new(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("no such tile variant"))
                .expect("body should be built"))
        })
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(LegendQuery {
        scale,
        mode,
        zoom,
        variant,
    }): Query<LegendQuery>,
) -> Response<Body> {
    let mode = mode.unwrap_or(LegendMode::Normal);

    let layers = match variant_layers(&state, variant.as_deref()) {
        Ok(layers) => layers,
        Err(response) => return *response,
    };

    let Some(render_request) = legend_render_request(
        id.as_str(),
        zoom,
        scale.unwrap_or(1f64),
        mode,
        layers.as_ref(),
    ) else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("legend item not found"))
            .expect("body should be built");
    };

    let rendered = match state.render_worker_pool.render(render_request).await {
        Ok(rendered) => rendered,
        Err(err) => {
            eprintln!("render failed: {err}");

            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("legend item render error"))
                .expect("body should be built");
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(
            "Content-Type",
            match mode {
                LegendMode::Normal => "image/png",
                LegendMode::Taginfo => "image/svg+xml",
            },
        )
        .body(Body::from(rendered.bytes))
        .expect("body should be built")
}
