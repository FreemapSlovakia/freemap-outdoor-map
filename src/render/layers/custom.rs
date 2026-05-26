use crate::render::{
    collision::Collision,
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        path_geom::{
            path_geometry, path_polygons, walk_geometry_line_strings, walk_geometry_points,
        },
        text::{TextOptions, draw_text},
        text_on_line::{TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::{LayerRenderError, LayerRenderResult},
    projectable::TileProjectable,
};
use cairo::{Context, LineCap, LineJoin};
use colorsys::{Rgb, RgbRatio};
use geo::{Geometry, InteriorPoint, Rect, Transform, Translate};
use geojson::Feature;
use proj::Proj;
use serde_json::Value;

struct FeatureProps {
    color: RgbRatio,
    stroke_opacity: f64,
    fill: Option<RgbRatio>,
    fill_opacity: Option<f64>,
    marker_color: Option<RgbRatio>,
    marker_color_opacity: Option<f64>,
    width: f64,
    name: Option<String>,
    line_join: Option<LineJoin>,
    line_cap: Option<LineCap>,
    dash_array: Option<Vec<f64>>,
}

fn parse_props(feature: &Feature) -> FeatureProps {
    let mut width = 3f64;
    let mut color = RgbRatio::new(1.0, 0.0, 1.0, 1.0);
    let mut stroke_opacity = 1f64;
    let mut fill: Option<RgbRatio> = None;
    let mut fill_opacity: Option<f64> = None;
    let mut marker_color: Option<RgbRatio> = None;
    let mut marker_color_opacity: Option<f64> = None;
    let mut name: Option<String> = None;
    let mut line_join: Option<LineJoin> = None;
    let mut line_cap: Option<LineCap> = None;
    let mut dash_array: Option<Vec<f64>> = None;

    if let Some(ref properties) = feature.properties {
        if let Some(Value::String(c)) = properties.get("stroke")
            && let Ok(rgb) = Rgb::from_hex_str(c)
        {
            color = rgb.as_ratio();
        }

        if let Some(Value::Number(o)) = properties.get("stroke-opacity")
            && let Some(v) = o.as_f64()
        {
            stroke_opacity = v;
        }

        if let Some(Value::String(c)) = properties.get("fill") {
            fill = Rgb::from_hex_str(c).ok().map(|rgb| rgb.as_ratio());
        }

        if let Some(Value::Number(o)) = properties.get("fill-opacity")
            && let Some(v) = o.as_f64()
        {
            fill_opacity = Some(v);
        }

        if let Some(Value::String(c)) = properties.get("marker-color") {
            marker_color = Rgb::from_hex_str(c).ok().map(|rgb| rgb.as_ratio());
        }

        if let Some(Value::Number(o)) = properties.get("marker-color-opacity")
            && let Some(v) = o.as_f64()
        {
            marker_color_opacity = Some(v);
        }

        if let Some(Value::String(n)) = properties.get("title")
            && !n.is_empty()
        {
            name.replace(n.clone());
        }

        if let Some(Value::Number(a)) = properties.get("stroke-width")
            && let Some(w) = a.as_f64()
        {
            width = w;
        }

        if let Some(Value::String(s)) = properties.get("stroke-linejoin") {
            line_join = match s.as_str() {
                "round" => Some(LineJoin::Round),
                "miter" => Some(LineJoin::Miter),
                "bevel" => Some(LineJoin::Bevel),
                _ => None,
            };
        }

        if let Some(Value::String(s)) = properties.get("stroke-linecap") {
            line_cap = match s.as_str() {
                "butt" => Some(LineCap::Butt),
                "round" => Some(LineCap::Round),
                "square" => Some(LineCap::Square),
                _ => None,
            };
        }

        if let Some(Value::Array(arr)) = properties.get("stroke-dasharray") {
            dash_array = Some(arr.iter().filter_map(|v| v.as_f64()).collect());
        }
    }

    FeatureProps {
        color,
        stroke_opacity,
        fill,
        fill_opacity,
        marker_color,
        marker_color_opacity,
        width,
        name,
        line_join,
        line_cap,
        dash_array,
    }
}

fn make_proj() -> Proj {
    // TODO lazy
    Proj::new_known_crs("EPSG:4326", "EPSG:3857", None).expect("projection 4326 -> 3857")
}

pub fn render_lines_polygons(
    ctx: &Ctx,
    context: &Context,
    features: &[Feature],
) -> LayerRenderResult {
    let proj = make_proj();

    let items = features
        .iter()
        .map(|f| {
            let mut geom: Geometry = Geometry::try_from(f.clone())?;
            geom.transform(&proj).unwrap();
            Ok((geom.project_to_tile(&ctx.tile_projector), parse_props(f)))
        })
        .collect::<Result<Vec<_>, LayerRenderError>>()?;

    context.save()?;

    // Pass 1: polygon fills (must come before strokes so borders render on top).
    for (geom, props) in &items {
        path_polygons(context, geom);
        let (fr, fg, fb, base_a) = match &props.fill {
            Some(fill) => (fill.r(), fill.g(), fill.b(), fill.a()),
            None => (
                props.color.r(),
                props.color.g(),
                props.color.b(),
                props.color.a() * 0.25,
            ),
        };
        context.set_source_rgba(fr, fg, fb, base_a * props.fill_opacity.unwrap_or(1.0));
        context.fill()?;
    }

    // Pass 2: strokes (polygon borders and lines).
    for (geom, props) in &items {
        let color = &props.color;
        path_geometry(context, geom);
        context.set_line_width(props.width);
        context.set_source_rgba(
            color.r(),
            color.g(),
            color.b(),
            color.a() * props.stroke_opacity,
        );
        context.set_line_join(props.line_join.unwrap_or(LineJoin::Round));
        context.set_line_cap(props.line_cap.unwrap_or(LineCap::Round));
        context.set_dash(props.dash_array.as_deref().unwrap_or(&[]), 0.0);
        context.stroke()?;
    }

    context.restore()?;

    Ok(())
}

pub fn render_points(
    ctx: &Ctx,
    context: &Context,
    features: &[Feature],
    collision: &mut Collision,
) -> LayerRenderResult {
    let proj = make_proj();

    for feature in features {
        let mut geom: Geometry = Geometry::try_from(feature.clone())?;

        geom.transform(&proj).unwrap();

        let geom = geom.project_to_tile(&ctx.tile_projector);

        let FeatureProps {
            color,
            marker_color,
            marker_color_opacity,
            ..
        } = parse_props(feature);

        let c = marker_color.unwrap_or(color);

        context.set_source_rgba(c.r(), c.g(), c.b(), c.a() * marker_color_opacity.unwrap_or(1.0));

        walk_geometry_points(&geom, &mut |point| -> cairo::Result<()> {
            let x = point.x();
            let y = point.y();
            let radius = 10f64;
            let h = radius * 2.2;
            let dy = radius * radius / h;
            let tx_sq = radius * radius - dy * dy;
            let tx = tx_sq.max(0.0).sqrt();

            context.new_sub_path();
            context.move_to(x, y);
            context.line_to(x - tx, y + (dy - h));
            context.arc(x, y - h, radius, dy.atan2(-tx), dy.atan2(tx));
            context.line_to(x, y);
            context.close_path();
            context.fill()?;

            // Register teardrop bbox so other layers respect its footprint.
            // TODO: check collision before rendering (currently always renders).
            collision.add(Rect::new((x - radius, y - h - radius), (x + radius, y)));

            Ok(())
        })?;
    }

    Ok(())
}

pub fn render_line_polygon_labels(
    ctx: &Ctx,
    context: &Context,
    features: &[Feature],
    collision: &mut Collision,
) -> LayerRenderResult {
    let proj = make_proj();

    for feature in features {
        let mut geom: Geometry = Geometry::try_from(feature.clone())?;

        geom.transform(&proj).unwrap();

        let geom = geom.project_to_tile(&ctx.tile_projector);

        if matches!(geom, Geometry::Point(_)) {
            continue;
        }

        let FeatureProps { name, .. } = parse_props(feature);

        let Some(name) = name else { continue };

        if matches!(geom, Geometry::LineString(_) | Geometry::MultiLineString(_)) {
            walk_geometry_line_strings(&geom, &mut |ls| {
                let _ = draw_text_on_line(
                    context,
                    ls,
                    &name,
                    Some(collision),
                    &TextOnLineOptions {
                        flo: FontAndLayoutOptions {
                            size: 15.0,
                            ..Default::default()
                        },
                        halo_width: 2.0,
                        ..Default::default()
                    },
                )?;
                cairo::Result::Ok(())
            })?;
        } else {
            let Some(point) = geom.interior_point() else {
                continue;
            };

            // TODO: render unconditionally; currently draw_text skips on collision
            let _ = draw_text(
                context,
                Some(collision),
                &point,
                &name,
                &TextOptions {
                    flo: FontAndLayoutOptions {
                        size: 15.0,
                        ..Default::default()
                    },
                    halo_width: 2.0,
                    ..Default::default()
                },
            );
        }
    }

    Ok(())
}

pub fn render_point_labels(
    ctx: &Ctx,
    context: &Context,
    features: &[Feature],
    collision: &mut Collision,
) -> LayerRenderResult {
    let proj = make_proj();

    for feature in features {
        let mut geom: Geometry = Geometry::try_from(feature.clone())?;

        geom.transform(&proj).unwrap();

        let geom = geom.project_to_tile(&ctx.tile_projector);

        let Geometry::Point(point) = geom else {
            continue;
        };

        let FeatureProps { name, .. } = parse_props(feature);

        let Some(name) = name else { continue };

        let point = point.translate(0.0, -44.0);

        // TODO: render unconditionally; currently draw_text skips on collision
        let _ = draw_text(
            context,
            Some(collision),
            &point,
            &name,
            &TextOptions {
                flo: FontAndLayoutOptions {
                    size: 15.0,
                    ..Default::default()
                },
                halo_width: 2.0,
                ..Default::default()
            },
        );
    }

    Ok(())
}
