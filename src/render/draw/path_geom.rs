use cairo::Context;
use cavalier_contours::polyline::{PlineSource, PlineSourceMut, PlineVertex, Polyline};
use geo::{Geometry, LineString, Point, Polygon};

pub fn path_geometry(context: &Context, geom: &Geometry) {
    walk_geometry_line_strings::<_, Result<(), ()>>(geom, &mut |line_string| {
        path_line_string(context, line_string);

        Ok(())
    })
    .expect("ok");
}

pub fn walk_geometry_line_strings<F, E>(geom: &Geometry, dl: &mut F) -> Result<(), E>
where
    F: FnMut(&LineString) -> Result<(), E>,
{
    match geom {
        Geometry::GeometryCollection(gc) => {
            for geometry in gc {
                walk_geometry_line_strings(geometry, dl)?;
            }
        }
        Geometry::Polygon(p) => {
            path_polygon(p, dl)?;
        }
        Geometry::MultiPolygon(mp) => {
            for p in mp {
                path_polygon(p, dl)?;
            }
        }
        Geometry::MultiLineString(mls) => {
            for ls in mls {
                dl(ls)?;
            }
        }
        Geometry::LineString(ls) => {
            dl(ls)?;
        }
        Geometry::Rect(r) => {
            dl(r.to_polygon().exterior())?;
        }
        Geometry::Triangle(r) => {
            dl(r.to_polygon().exterior())?;
        }
        Geometry::Line(line) => {
            dl(&LineString::new(vec![line.start, line.end]))?;
        }
        Geometry::Point(_) | Geometry::MultiPoint(_) => {}
    }

    Ok(())
}

pub fn path_polygons(context: &Context, geom: &Geometry) {
    walk_geometry_polygons::<_, Result<(), ()>>(geom, &mut |line_string| {
        path_line_string(context, line_string);

        Ok(())
    })
    .expect("ok")
}

pub fn walk_geometry_polygons<F, E>(geom: &Geometry, dl: &mut F) -> Result<(), E>
where
    F: FnMut(&LineString) -> Result<(), E>,
{
    match geom {
        Geometry::GeometryCollection(gc) => {
            for geometry in gc {
                walk_geometry_polygons(geometry, dl)?;
            }
        }
        Geometry::Polygon(p) => {
            path_polygon(p, dl)?;
        }
        Geometry::MultiPolygon(mp) => {
            for p in mp {
                path_polygon(p, dl)?;
            }
        }
        Geometry::Rect(r) => {
            dl(r.to_polygon().exterior())?;
        }
        Geometry::Triangle(r) => {
            dl(r.to_polygon().exterior())?;
        }
        _ => {}
    }

    Ok(())
}

pub fn walk_geometry_points<F, E>(geom: &Geometry, dl: &mut F) -> Result<(), E>
where
    F: FnMut(&Point) -> Result<(), E>,
{
    match geom {
        Geometry::GeometryCollection(gc) => {
            for geometry in gc {
                walk_geometry_points(geometry, dl)?;
            }
        }
        Geometry::Point(p) => {
            dl(p)?;
        }
        Geometry::MultiPoint(mp) => {
            for p in mp {
                dl(p)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn path_polygon<F, E>(poly: &Polygon, dl: &mut F) -> Result<(), E>
where
    F: FnMut(&LineString) -> Result<(), E>,
{
    dl(poly.exterior())?;

    for ring in poly.interiors() {
        dl(ring)?;
    }

    Ok(())
}

pub fn path_line_string(context: &Context, line_string: &LineString) {
    let len = line_string.0.len();

    for (i, p) in line_string.into_iter().enumerate() {
        if i == 0 {
            context.move_to(p.x, p.y);
        } else if i == len - 1 && &line_string.0[0] == p {
            context.close_path();
        } else {
            context.line_to(p.x, p.y);
        }
    }
}

pub fn path_line_string_with_offset(context: &Context, line_string: &LineString, offset: f64) {
    let mut polyline = Polyline::new();

    for p in line_string {
        polyline.add_vertex(PlineVertex::new(p.x, p.y, 0.0));
    }

    let polyline = polyline.remove_repeat_pos(1e-2).unwrap_or(polyline);

    for pc in polyline.parallel_offset(offset) {
        let mut first = true;
        let mut p1 = (0.0, 0.0);
        let mut prev_bulge = 0.0f64;

        for v in pc.vertex_data {
            if first {
                context.move_to(v.x, v.y);
                first = false;
                p1 = (v.x, v.y);
            } else {
                let p2 = (v.x, v.y);

                if prev_bulge == 0.0 {
                    context.line_to(p2.0, p2.1);
                } else {
                    let theta = 4.0 * prev_bulge.atan();
                    let dist = (p2.0 - p1.0).hypot(p2.1 - p1.1);
                    let radius = dist / (2.0 * (theta / 2.0).sin());

                    // Calculate center of the arc
                    let mx = (p1.0 + p2.0) / 2.0;
                    let my = (p1.1 + p2.1) / 2.0;
                    let l = (dist / 2.0).mul_add(-(dist / 2.0), radius.powi(2)).sqrt();
                    let direction = if prev_bulge > 0.0 { 1.0 } else { -1.0 };
                    let ox = mx - direction * l * (p2.1 - p1.1) / dist;
                    let oy = my + direction * l * (p2.0 - p1.0) / dist;

                    // Calculate start and end angles
                    let mut start_angle =
                        5.0f64.mul_add(std::f64::consts::PI, (p1.1 - oy).atan2(p1.0 - ox));
                    let mut end_angle =
                        5.0f64.mul_add(std::f64::consts::PI, (p2.1 - oy).atan2(p2.0 - ox));

                    if prev_bulge > 0.0 {
                        start_angle += std::f64::consts::PI;
                        end_angle += std::f64::consts::PI;

                        while end_angle < start_angle {
                            end_angle += 2.0 * std::f64::consts::PI;
                        }

                        let mut angle = start_angle;

                        while angle < end_angle {
                            angle += std::f64::consts::PI / 10.0;

                            context.line_to(
                                radius.mul_add(angle.cos(), ox),
                                radius.mul_add(angle.sin(), oy),
                            );
                        }
                    } else {
                        while end_angle > start_angle {
                            end_angle -= 2.0 * std::f64::consts::PI;
                        }

                        let mut angle = start_angle;

                        while angle > end_angle {
                            angle -= std::f64::consts::PI / 10.0;

                            context.line_to(
                                radius.mul_add(angle.cos(), ox),
                                radius.mul_add(angle.sin(), oy),
                            );
                        }
                    }
                    // if prev_bulge > 0.0 {
                    //     context.arc(ox, oy, radius.abs(), start_angle, end_angle);
                    // } else {
                    //     context.arc_negative(ox, oy, radius.abs(), start_angle, end_angle);
                    // }
                }

                p1 = p2;
            }

            prev_bulge = v.bulge;
        }
    }
}
