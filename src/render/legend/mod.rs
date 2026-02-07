mod ctx_ext;

use std::collections::HashMap;

use geo::{Coord, Point, Rect, polygon};
use serde::Serialize;

use crate::render::{ImageFormat, LegendValue, RenderRequest};

#[derive(Clone, Serialize)]
pub struct LegendMeta {
    pub category: String,
    pub tags: HashMap<String, String>,
}

pub fn legend_metadata() -> Vec<(String, LegendMeta)> {
    Vec::from([(
        "police".to_string(),
        LegendMeta {
            category: "institution".to_string(),
            tags: HashMap::new(),
        },
    )])
}

pub fn legend_render_request(id: &str, scale: f64) -> Option<RenderRequest> {
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

    let mut legend_map: HashMap<String, Vec<HashMap<String, LegendValue>>> = HashMap::new();

    match id {
        "police" => {
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

            legend_map.insert("features".to_string(), vec![legend_feature]);
        }
        "meadow" => {
            let mut legend_feature = HashMap::new();

            legend_feature.insert(
                "geometry".to_string(),
                LegendValue::Geometry(geo::Geometry::Polygon(polygon![
                    (x: -100.0, y: -100.0),
                    (x: -100.0, y: 100.0),
                    (x: 100.0, y: 100.0),
                    (x: -100.0, y: -100.0)
                ])),
            );
            legend_feature.insert(
                "type".to_string(),
                LegendValue::String("meadow".to_string()),
            );
            legend_feature.insert("name".to_string(), LegendValue::String("Test".to_string()));

            legend_map.insert("landcover".to_string(), vec![legend_feature]);
        }
        _ => return None,
    }

    let mut render_request = RenderRequest::new(bbox, zoom, scale, ImageFormat::Jpeg);
    render_request.legend = Some(legend_map);

    Some(render_request)
}
