use cavalier_contours::polyline::{PlineSource, PlineSourceMut, PlineVertex, Polyline};
use geo::{Coord, LineString};

pub fn offset_line_string(line_string: &LineString, offset: f64) -> LineString {
    let _span = tracy_client::span!("offset_line::offset_line_string");

    let mut result = Vec::<Coord>::new();

    let mut polyline = Polyline::new();

    for p in line_string {
        polyline.add_vertex(PlineVertex::new(p.x, p.y, 0.0));
    }

    let polyline = polyline.remove_repeat_pos(1e-2).unwrap_or(polyline);

    for pc in polyline.parallel_offset(offset) {
        if let Some(polyline) = pc.arcs_to_approx_lines(3.0) {
            for vertex in polyline.vertex_data {
                result.push(Coord {
                    x: vertex.x,
                    y: vertex.y,
                });
            }
        }
    }

    LineString(result)
}
