use crate::{
    app::server::app_state::AppState,
    render::{LegendMeta, LegendMode, legend_metadata, legend_render_request},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{Response, StatusCode},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LegendQuery {
    scale: Option<f64>,
    mode: Option<LegendMode>,
}

pub(crate) async fn get_metadata() -> Json<Vec<LegendMeta<'static>>> {
    Json(legend_metadata())
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(LegendQuery { scale, mode }): Query<LegendQuery>,
) -> Response<Body> {
    let mode = mode.unwrap_or(LegendMode::Normal);

    let Some(render_request) = legend_render_request(id.as_str(), scale.unwrap_or(1f64), mode)
    else {
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
        .body(Body::from(rendered))
        .expect("body should be built")
}
