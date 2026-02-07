use std::collections::HashMap;

use crate::{
    app::server::app_state::AppState,
    render::{ImageFormat, LegendValue, RenderRequest},
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{Response, StatusCode},
};
use geo::{Coord, Point, Rect, polygon};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LegendScale {
    scale: f64,
}

const CATEGORIES: &[(&str, &[&str])] = &[
    ("communications", &[]),
    ("railway", &[]),
    ("landcover", &[]),
    ("borders", &[]),
    ("accomodation", &[]),
    ("natural_poi", &[]),
    ("gastro_poi", &[]),
    ("water", &[]),
    ("institution", &[]),
    ("poi", &[]),
    ("terrain", &[]),
    ("other", &[]),
];

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<u16>,
    Query(LegendScale { scale }): Query<LegendScale>,
) -> Response<Body> {
    let zoom = 16;

    let bbox = Rect::new(
        Coord {
            x: -1000.0,
            y: -1000.0,
        },
        Coord {
            x: 1000.0,
            y: 1000.0,
        },
    );

    let mut legend_feature = HashMap::new();

    legend_feature.insert(
        "geometry".to_string(),
        LegendValue::Point(Point::new(1.0, 1.0)),
    );
    legend_feature.insert(
        "type".to_string(),
        LegendValue::String("police".to_string()),
    );
    legend_feature.insert("n".to_string(), LegendValue::String("Test".to_string()));
    legend_feature.insert("h".to_string(), LegendValue::Hstore(HashMap::new()));

    let mut legend_map = HashMap::new();

    legend_map.insert("features".to_string(), vec![legend_feature]);

    let mut legend_feature = HashMap::new();

    legend_feature.insert(
        "geometry".to_string(),
        LegendValue::Geometry(geo::Geometry::Polygon(polygon![(x: -100.0, y: -100.0), (x: -100.0, y: 100.0), (x: 100.0, y: 100.0), (x: -100.0, y: -100.0)])),
    );
    legend_feature.insert(
        "type".to_string(),
        LegendValue::String("meadow".to_string()),
    );
    legend_feature.insert("name".to_string(), LegendValue::String("Test".to_string()));

    legend_map.insert("landcover".to_string(), vec![legend_feature]);

    let mut render_request = RenderRequest::new(bbox, zoom, scale, ImageFormat::Jpeg);

    render_request.legend = Some(legend_map);

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
