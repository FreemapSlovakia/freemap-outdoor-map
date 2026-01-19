use crate::app::{app_state::AppState, tiles::serve_tile};
use axum::{
    body::Body,
    extract::{Query, State},
    http::{Response, StatusCode},
};
use std::collections::HashMap;

const CAPABILITIES_XML: &str = include_str!("service_capabilities.xml");

pub(crate) async fn service_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    if get_param(&params, "SERVICE") != Some("WMTS")
        || (get_param(&params, "VERSION") != Some("1.0.0"))
    {
        return bad_request();
    }

    match get_param(&params, "REQUEST") {
        Some("GetTile") => {
            let layer = get_param(&params, "LAYER");
            let tile_matrix_set = get_param(&params, "TILEMATRIXSET");
            let format = get_param(&params, "FORMAT");
            let tile_matrix = get_param(&params, "TILEMATRIX");
            let tile_col = get_param(&params, "TILECOL");
            let tile_row = get_param(&params, "TILEROW");

            let (scale, ext) = match (layer, tile_matrix_set, format) {
                (Some("freemap_outdoor"), Some("webmercator"), Some("image/jpeg")) => (1.0, "jpeg"),
                (Some("freemap_outdoor_2x"), Some("webmercator_2x"), Some("image/jpeg")) => {
                    (2.0, "jpeg")
                }
                _ => return bad_request(),
            };

            let zoom = match tile_matrix.and_then(|v| v.parse::<u32>().ok()) {
                Some(value) => value,
                None => return bad_request(),
            };

            let x = match tile_col.and_then(|v| v.parse::<u32>().ok()) {
                Some(value) => value,
                None => return bad_request(),
            };

            let y = match tile_row.and_then(|v| v.parse::<u32>().ok()) {
                Some(value) => value,
                None => return bad_request(),
            };

            serve_tile(&state, zoom, x, y, scale, Some(ext)).await
        }
        Some("GetCapabilities") => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/xml")
            .body(Body::from(CAPABILITIES_XML))
            .expect("capabilities body"),
        _ => bad_request(),
    }
}

fn get_param<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.get(key).map(|value| value.as_str())
}

fn bad_request() -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .expect("bad request body")
}
