use crate::render::ctx::Ctx;
use cairo::{Matrix, RecordingSurface, Result, SurfacePattern};
use geo::{Coord, LineString};

fn get_perpendicular(dx: f64, dy: f64, length: f64, stroke_width: f64) -> (f64, f64) {
    (
        (-dy / length) * stroke_width / 2.0,
        (dx / length) * stroke_width / 2.0,
    )
}

fn get_intersection1(p1: Coord, p2: Coord, p3: Coord, p4: Coord) -> Option<Coord> {
    let s1_x = p2.x - p1.x;
    let s1_y = p2.y - p1.y;
    let s2_x = p4.x - p3.x;
    let s2_y = p4.y - p3.y;

    let denom = s1_x.mul_add(s2_y, -(s2_x * s1_y));

    if denom.abs() < f64::EPSILON {
        return None;
    }

    let s = s1_x.mul_add(p1.y - p3.y, -(s1_y * (p1.x - p3.x))) / denom;

    let t = s2_x.mul_add(p1.y - p3.y, -(s2_y * (p1.x - p3.x))) / denom;

    if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
        Some(Coord {
            x: t.mul_add(s1_x, p1.x),
            y: t.mul_add(s1_y, p1.y),
        })
    } else {
        None
    }
}

fn get_intersection(p1: Coord, p2: Coord, p3: Coord, p4: Coord) -> Option<Coord> {
    let s1_x = p2.x - p1.x;
    let s1_y = p2.y - p1.y;
    let s2_x = p4.x - p3.x;
    let s2_y = p4.y - p3.y;

    let denom = s1_x.mul_add(s2_y, -(s2_x * s1_y));

    if denom.abs() < f64::EPSILON {
        return None;
    }

    let s = s1_x.mul_add(p1.y - p3.y, -(s1_y * (p1.x - p3.x))) / denom;

    Some(Coord {
        x: s.mul_add(s2_x, p3.x),
        y: s.mul_add(s2_y, p3.y),
    })
}

fn should_use_bevel_join(
    p0: Coord,
    p1: Coord,
    p2: Coord,
    stroke_width: f64,
    miter_limit: f64,
) -> bool {
    let v1 = (p1.x - p0.x, p1.y - p0.y);
    let v2 = (p2.x - p1.x, p2.y - p1.y);

    let len_v1 = v1.0.hypot(v1.1);
    let len_v2 = v2.0.hypot(v2.1);

    let v1_norm = (v1.0 / len_v1, v1.1 / len_v1);
    let v2_norm = (v2.0 / len_v2, v2.1 / len_v2);

    let dot = v1_norm.0.mul_add(v2_norm.0, v1_norm.1 * v2_norm.1);
    let angle = dot.clamp(-1.0, 1.0).acos();

    let miter_length = stroke_width / (2.0 * (angle / 2.0).sin());

    miter_length > miter_limit * stroke_width
}

fn cross_product(v1: Coord, v2: Coord, v3: Coord) -> f64 {
    (v2.x - v1.x).mul_add(v3.y - v2.y, -((v2.y - v1.y) * (v3.x - v2.x)))
}

fn compute_corners(p0: Coord, p1: Coord, stroke_width: f64) -> (Coord, Coord, Coord, Coord) {
    let dx0 = p1.x - p0.x;
    let dy0 = p1.y - p0.y;
    let length0 = dx0.hypot(dy0);
    let perp0 = get_perpendicular(dx0, dy0, length0, stroke_width);

    (
        Coord {
            x: p0.x + perp0.0,
            y: p0.y + perp0.1,
        },
        Coord {
            x: p0.x - perp0.0,
            y: p0.y - perp0.1,
        },
        Coord {
            x: p1.x - perp0.0,
            y: p1.y - perp0.1,
        },
        Coord {
            x: p1.x + perp0.0,
            y: p1.y + perp0.1,
        },
    )
}

pub fn draw_line_pattern(
    ctx: &Ctx,
    line_string: &LineString,
    miter_limit: f64,
    sample: &RecordingSurface,
) -> cairo::Result<()> {
    draw_line_pattern_scaled(ctx, line_string, miter_limit, 1.0, sample)
}

pub fn draw_line_pattern_scaled(
    ctx: &Ctx,
    line_string: &LineString,
    miter_limit: f64,
    scale: f64,
    sample: &RecordingSurface,
) -> Result<()> {
    let mut vertices = line_string.0.clone();

    vertices.reverse();

    let len = vertices.len();

    if len < 2 {
        return Ok(());
    }

    let pattern = SurfacePattern::create(sample);

    let rect = sample.extents().expect("extents");

    let (width, height) = (rect.width(), rect.height());

    let stroke_width = height * scale;

    let context = &ctx.context;

    let mut dist = 0.0;

    let is_closed = vertices.first() == vertices.last();

    context.push_group();

    // context.set_operator(cairo::Operator::Source);

    for i in 0..len - 1 {
        let p1 = vertices[i];
        let p2 = vertices[i + 1];

        let length = (p2.x - p1.x).hypot(p2.y - p1.y);

        let min_x = -width * 10.0;
        let min_y = -height * 10.0;
        let max_x = width.mul_add(10.0, ctx.size.width as f64);
        let max_y = height.mul_add(10.0, ctx.size.height as f64);

        if p1.x < min_x && p2.x < min_x
            || p1.y < min_y && p2.y < min_y
            || p1.x > max_x && p2.x > max_x
            || p1.y > max_y && p2.y > max_y
        {
            dist += length;

            continue;
        }

        let (mut corner1, mut corner2, mut corner3, mut corner4) =
            compute_corners(p1, p2, stroke_width);

        let mut extra_corner1: Option<Coord> = None;
        let mut extra_corner2: Option<Coord> = None;

        let mut use_corner1 = true;
        let mut use_corner2 = true;
        let mut use_corner3 = true;
        let mut use_corner4 = true;

        if is_closed || i > 0 {
            let p0 = vertices[if i == 0 { len - 1 } else { i } - 1];

            let (prev_corner1, prev_corner2, prev_corner3, prev_corner4) =
                compute_corners(p0, p1, stroke_width);

            let cp = cross_product(p0, p1, p2);

            let bevel = should_use_bevel_join(p0, p1, p2, stroke_width, miter_limit);

            if !bevel {
                extra_corner1 = Some(if cp < 0.0 {
                    Coord {
                        x: (corner1.x + prev_corner4.x) / 2.0,
                        y: (corner1.y + prev_corner4.y) / 2.0,
                    }
                } else {
                    Coord {
                        x: (corner2.x + prev_corner3.x) / 2.0,
                        y: (corner2.y + prev_corner3.y) / 2.0,
                    }
                });
            }

            if let Some(intersection) =
                get_intersection1(corner1, corner4, prev_corner1, prev_corner2)
            {
                corner1 = intersection;
            } else if let Some(intersection) =
                get_intersection1(corner3, corner4, prev_corner1, prev_corner4)
            {
                corner1 = intersection;
                use_corner4 = false;
            } else if let Some(intersection) = (bevel || cp > 0.0)
                .then(|| get_intersection(corner1, corner4, prev_corner1, prev_corner4))
                .flatten()
            {
                corner1 = intersection;
            }

            if let Some(intersection) =
                get_intersection1(corner2, corner3, prev_corner1, prev_corner2)
            {
                corner2 = intersection;
            } else if let Some(intersection) =
                get_intersection1(corner3, corner4, prev_corner2, prev_corner3)
            {
                corner2 = intersection;
                use_corner3 = false;
            } else if let Some(intersection) = (bevel || cp < 0.0)
                .then(|| get_intersection(corner2, corner3, prev_corner2, prev_corner3))
                .flatten()
            {
                corner2 = intersection;
            }
        }

        if is_closed || i < len - 2 {
            let p3 = vertices[if i == len - 2 { 1 } else { i + 2 }];

            let (next_corner1, next_corner2, next_corner3, next_corner4) =
                compute_corners(p2, p3, stroke_width);

            let cp = cross_product(p1, p2, p3);

            let bevel = should_use_bevel_join(p1, p2, p3, stroke_width, miter_limit);

            if !bevel {
                extra_corner2 = Some(if cp < 0.0 {
                    Coord {
                        x: (corner4.x + next_corner1.x) / 2.0,
                        y: (corner4.y + next_corner1.y) / 2.0,
                    }
                } else {
                    Coord {
                        x: (corner3.x + next_corner2.x) / 2.0,
                        y: (corner3.y + next_corner2.y) / 2.0,
                    }
                });
            }

            if let Some(intersection) =
                get_intersection1(corner1, corner2, next_corner2, next_corner3)
            {
                use_corner2 = false;
                corner3 = intersection;
            } else if let Some(intersection) =
                get_intersection1(corner2, corner3, next_corner3, next_corner4)
            {
                corner3 = intersection;
            } else if let Some(intersection) = (bevel || cp < 0.0)
                .then(|| get_intersection(corner2, corner3, next_corner2, next_corner3))
                .flatten()
            {
                corner3 = intersection;
            }

            if let Some(intersection) =
                get_intersection1(corner1, corner2, next_corner1, next_corner4)
            {
                use_corner1 = false;
                corner4 = intersection;
            } else if let Some(intersection) =
                get_intersection1(corner1, corner4, next_corner3, next_corner4)
            {
                corner4 = intersection;
            } else if let Some(intersection) = (bevel || cp > 0.0)
                .then(|| get_intersection(corner1, corner4, next_corner1, next_corner4))
                .flatten()
            {
                corner4 = intersection;
            }
        }

        context.new_path();

        if use_corner1 {
            context.line_to(corner1.x, corner1.y);
        }

        if let Some(ec) = extra_corner1 {
            context.line_to(ec.x, ec.y);
        }

        if use_corner2 {
            context.line_to(corner2.x, corner2.y);
        }

        if use_corner3 {
            context.line_to(corner3.x, corner3.y);
        }

        if let Some(ec) = extra_corner2 {
            context.line_to(ec.x, ec.y);
        }

        if use_corner4 {
            context.line_to(corner4.x, corner4.y);
        }

        context.close_path();

        let (x1, y1, x2, y2) = context.path_extents()?;

        if !(x2 < 0.0 || x1 > ctx.size.width as f64 || y2 < 0.0 || y1 > ctx.size.width as f64) {
            // context.set_line_width(1.0);
            // context.set_dash(&[], 0.0);
            // context.set_source_rgb(0.0, 0.0, 0.0);
            // context.stroke_preserve()?;

            let mut matrix = Matrix::identity();

            matrix.translate(width / 2.0 + ((dist / scale) % width), height / 2.0);

            matrix.scale(1.0 / scale, 1.0 / scale);

            matrix.rotate((p1.y - p2.y).atan2(p2.x - p1.x));

            matrix.translate(-p1.x, -p1.y);

            pattern.set_matrix(matrix);

            pattern.set_extend(cairo::Extend::Repeat);

            context.set_source(&pattern)?;

            context.fill()?;
        }

        dist += length;
    }

    context.pop_group_to_source()?;

    context.paint()?;

    Ok(())
}
