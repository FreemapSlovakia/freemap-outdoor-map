use crate::render::draw::path_geom::path_line_string;
use cairo::Context;
use geo::{Coord, LineString};

// https://github.com/ghaerr/agg-2.6/blob/master/agg-src/src/agg_vcgen_smooth_poly1.cpp
// https://agg.sourceforge.net/antigrain.com/research/bezier_interpolation/index.html
pub fn path_smooth_bezier_spline(context: &Context, line_string: &LineString, smooth_value: f64) {
    if smooth_value == 0.0 {
        path_line_string(context, line_string);

        return;
    }

    let mut points: Vec<Coord> = line_string.0.clone();

    let mut len = points.len();

    assert!(len >= 2, "At least two points are required");

    let off = if points[0] == points[len - 1] {
        points.pop();

        len -= 1;

        1
    } else {
        0
    };

    if len < 2 {
        return;
    }

    let Coord { x, y } = points[off];

    context.move_to(x, y);

    if len < 3 {
        let Coord { x: x1, y: y1 } = points[1];

        context.line_to(x1, y1);

        return;
    }

    for i in off..len - 1 + off * 4 {
        let Coord { x: x1, y: y1 } = points[i % len];
        let Coord { x: x2, y: y2 } = points[(i + 1) % len];

        let len2 = (x2 - x1).hypot(y2 - y1);
        let xc2 = f64::midpoint(x1, x2);
        let yc2 = f64::midpoint(y1, y2);

        let ctrl1 = if off == 0 && i == 0 {
            Coord { x: x1, y: y1 }
        } else {
            let Coord { x: x0, y: y0 } = points[(i - 1) % len];
            let len1 = (x1 - x0).hypot(y1 - y0);
            let k1 = len1 / (len1 + len2);
            let xc1 = f64::midpoint(x0, x1);
            let yc1 = f64::midpoint(y0, y1);
            let xm1 = (xc2 - xc1).mul_add(k1, xc1);
            let ym1 = (yc2 - yc1).mul_add(k1, yc1);

            Coord {
                x: (xc2 - xm1).mul_add(smooth_value, xm1) + x1 - xm1,
                y: (yc2 - ym1).mul_add(smooth_value, ym1) + y1 - ym1,
            }
        };

        let ctrl2 = if off == 0 && i == len - 2 {
            Coord { x: x2, y: y2 }
        } else {
            let Coord { x: x3, y: y3 } = points[(i + 2) % len];
            let len3 = (x3 - x2).hypot(y3 - y2);
            let k2 = len2 / (len2 + len3);
            let xc3 = f64::midpoint(x2, x3);
            let yc3 = f64::midpoint(y2, y3);
            let xm2 = (xc3 - xc2).mul_add(k2, xc2);
            let ym2 = (yc3 - yc2).mul_add(k2, yc2);

            Coord {
                x: (xc2 - xm2).mul_add(smooth_value, xm2) + x2 - xm2,
                y: (yc2 - ym2).mul_add(smooth_value, ym2) + y2 - ym2,
            }
        };

        context.curve_to(ctrl1.x, ctrl1.y, ctrl2.x, ctrl2.y, x2, y2);
    }
}

/// Rounds each corner with a quadratic curve controlled by the vertex, starting and ending
/// `cut` (0..=0.5) of the way along its two segments; 0.5 is a quadratic B-spline. Unlike the
/// interpolating spline above the curve never bulges past the polyline, so neighbouring
/// contours stay apart.
pub fn path_rounded_corners(context: &Context, line_string: &LineString, cut: f64) {
    let mut points: &[Coord] = &line_string.0;

    let closed = points.len() > 3 && points[0] == points[points.len() - 1];

    if closed {
        points = &points[..points.len() - 1];
    }

    let len = points.len();

    if len < 3 {
        path_line_string(context, line_string);

        return;
    }

    let lerp = |a: Coord, b: Coord, t: f64| Coord {
        x: (b.x - a.x).mul_add(t, a.x),
        y: (b.y - a.y).mul_add(t, a.y),
    };

    let range = if closed { 0..len } else { 1..len - 1 };

    if !closed {
        context.move_to(points[0].x, points[0].y);
    }

    for i in range {
        let ctrl = points[i];
        let from = lerp(ctrl, points[(i + len - 1) % len], cut);
        let to = lerp(ctrl, points[(i + 1) % len], cut);

        if closed && i == 0 {
            context.move_to(from.x, from.y);
        } else {
            context.line_to(from.x, from.y);
        }

        // Cairo only draws cubics; this is the exact cubic form of the quadratic.
        let c1 = lerp(from, ctrl, 2.0 / 3.0);
        let c2 = lerp(to, ctrl, 2.0 / 3.0);

        context.curve_to(c1.x, c1.y, c2.x, c2.y, to.x, to.y);
    }

    if closed {
        context.close_path();
    } else {
        context.line_to(points[len - 1].x, points[len - 1].y);
    }
}
