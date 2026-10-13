use cavalier_contours::polyline::{PlineSource, PlineSourceMut, PlineVertex, Polyline};
use geo::{Coord, LineString};

fn offset_parts(line_string: &LineString, offset: f64) -> impl Iterator<Item = Polyline> {
    let mut polyline = Polyline::new();

    for p in line_string {
        polyline.add_vertex(PlineVertex::new(p.x, p.y, 0.0));
    }

    let polyline = polyline.remove_repeat_pos(1e-2).unwrap_or(polyline);

    polyline
        .parallel_offset(offset)
        .into_iter()
        .filter_map(|pc| pc.arcs_to_approx_lines(3.0))
}

fn to_coords(polyline: Polyline) -> impl Iterator<Item = Coord> {
    polyline.vertex_data.into_iter().map(|vertex| Coord {
        x: vertex.x,
        y: vertex.y,
    })
}

pub fn offset_line_string(line_string: &LineString, offset: f64) -> LineString {
    let _span = tracy_client::span!("offset_line::offset_line_string");

    LineString(
        offset_parts(line_string, offset)
            .flat_map(to_coords)
            .collect(),
    )
}

/// The offset can split into several disjoint parts; drawing them as one line would
/// join their ends with a straight segment.
pub fn offset_line_string_parts(line_string: &LineString, offset: f64) -> Vec<LineString> {
    let _span = tracy_client::span!("offset_line::offset_line_string_parts");

    offset_parts(line_string, offset)
        .map(|polyline| LineString(to_coords(polyline).collect()))
        .collect()
}
