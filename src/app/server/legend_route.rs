use crate::{
    app::server::app_state::AppState,
    render::{LegendMeta, legend_metadata, legend_render_request},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{Response, StatusCode},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LegendScale {
    scale: f64,
}

pub(crate) async fn get_metadata() -> Json<Vec<(String, LegendMeta)>> {
    Json(legend_metadata())
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(LegendScale { scale }): Query<LegendScale>,
) -> Response<Body> {
    let Some(render_request) = legend_render_request(id.as_str(), scale) else {
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
        .header("Content-Type", "image/jpeg")
        .body(Body::from(rendered))
        .expect("body should be built")
}
