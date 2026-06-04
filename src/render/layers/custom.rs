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
use cairo::{Context, LineCap, LineJoin, Rectangle};
use colorsys::{Rgb, RgbRatio};
use geo::{Geometry, InteriorPoint, Rect, Transform, Translate};
use geojson::Feature;
use gio::glib;
use proj::Proj;
use serde_json::Value;

/// Rendered width (in tile/CSS pixels) of a drawing-point marker. All marker
/// shapes share viewBox width 310 and are scaled to this width; height follows
/// from each SVG's aspect ratio. The render context is already scaled by the
/// request's `scale`, so no extra scaling is needed here. Matches the ~30 px
/// in-app marker.
const MARKER_WIDTH: f64 = 30.0;

/// Styling for line and polygon features (simplestyle `stroke`/`fill`/…).
struct LinePolygonProps {
    color: RgbRatio,
    stroke_opacity: f64,
    fill: Option<RgbRatio>,
    fill_opacity: Option<f64>,
    width: f64,
    line_join: Option<LineJoin>,
    line_cap: Option<LineCap>,
    dash_array: Option<Vec<f64>>,
}

/// Styling for drawing-point features. The marker is fully described by its
/// self-contained `marker-svg`; points have no simplestyle color of their own.
struct PointProps {
    marker_svg: Option<String>,
    name: Option<String>,
}

/// The optional `title` label, shared by every geometry type.
fn parse_title(feature: &Feature) -> Option<String> {
    feature
        .properties
        .as_ref()?
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_line_polygon_props(feature: &Feature) -> LinePolygonProps {
    let mut width = 3f64;
    let mut color = RgbRatio::new(1.0, 0.0, 1.0, 1.0);
    let mut stroke_opacity = 1f64;
    let mut fill: Option<RgbRatio> = None;
    let mut fill_opacity: Option<f64> = None;
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
            dash_array = Some(arr.iter().filter_map(serde_json::Value::as_f64).collect());
        }
    }

    LinePolygonProps {
        color,
        stroke_opacity,
        fill,
        fill_opacity,
        width,
        line_join,
        line_cap,
        dash_array,
    }
}

fn parse_point_props(feature: &Feature) -> PointProps {
    let marker_svg = feature
        .properties
        .as_ref()
        .and_then(|p| p.get("marker-svg"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    PointProps {
        marker_svg,
        name: parse_title(feature),
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
            geom.transform(&proj).expect("geometry transformed");
            Ok((
                geom.project_to_tile(&ctx.tile_projector),
                parse_line_polygon_props(f),
            ))
        })
        .collect::<Result<Vec<_>, LayerRenderError>>()?;

    context.save()?;

    // Pass 1: polygon fills (must come before strokes so borders render on top).
    for (geom, props) in &items {
        path_polygons(context, geom);
        let (fr, fg, fb, base_a) = props.fill.as_ref().map_or_else(
            || {
                (
                    props.color.r(),
                    props.color.g(),
                    props.color.b(),
                    props.color.a() * 0.25,
                )
            },
            |fill| (fill.r(), fill.g(), fill.b(), fill.a()),
        );
        context.set_source_rgba(fr, fg, fb, base_a * props.fill_opacity.unwrap_or(1.0));
        context.fill()?;
    }

    // Pass 2: strokes (polygon borders and lines).
    for (geom, props) in &items {
        path_geometry(context, geom);

        context.set_line_width(props.width);

        let color = &props.color;
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

/// Rasterize an inline `marker-svg` (the entire self-contained marker — shape,
/// color, opacity and glyph) and paint it so the viewBox center lands on
/// `(x, y)`. The marker is scaled to a fixed width; height follows from its
/// aspect ratio (pins are taller, with transparent padding below the tip so the
/// tip ends up at center). Returns the rendered height on success, or `None`
/// when the SVG cannot be loaded (so the caller can fall back to a default
/// marker).
fn render_marker_svg(context: &Context, svg: &str, x: f64, y: f64) -> Option<f64> {
    let bytes = glib::Bytes::from_owned(svg.as_bytes().to_vec());

    let stream = gio::MemoryInputStream::from_bytes(&bytes);

    let handle = rsvg::Loader::new()
        .read_stream(&stream, None::<&gio::File>, None::<&gio::Cancellable>)
        .ok()?;

    let renderer = rsvg::CairoRenderer::new(&handle);

    let (sw, sh) = renderer.intrinsic_size_in_pixels()?;

    if sw <= 0.0 || sh <= 0.0 {
        return None;
    }

    let height = MARKER_WIDTH * sh / sw;

    let left = x - MARKER_WIDTH / 2.0;
    let top = y - height / 2.0;

    renderer
        .render_document(context, &Rectangle::new(left, top, MARKER_WIDTH, height))
        .ok()?;

    Some(height)
}

/// Rendered height of a `marker-svg` (scaled to `MARKER_WIDTH`), derived from
/// the root element's `width`/`height` attributes. Used to position the label
/// above the marker without re-rasterizing.
fn marker_svg_height(svg: &str) -> Option<f64> {
    let el = xmltree::Element::parse(svg.as_bytes()).ok()?;

    let w: f64 = el.attributes.get("width")?.parse().ok()?;
    let h: f64 = el.attributes.get("height")?.parse().ok()?;

    if w <= 0.0 {
        return None;
    }

    Some(MARKER_WIDTH * h / w)
}

/// Draw the default marker (a teardrop pin) for point features that carry no
/// `marker-svg` — e.g. points imported from tracks or search. Its tip sits on
/// `(x, y)`. Point features have no color of their own, so it uses the app's
/// default marker color (`#d00000`).
fn draw_default_marker(context: &Context, x: f64, y: f64) -> cairo::Result<f64> {
    let radius = 10f64;
    let h = radius * 2.2;
    let dy = radius * radius / h;
    let tx = radius.mul_add(radius, -(dy * dy)).max(0.0).sqrt();

    context.set_source_rgb(0.815_686, 0.0, 0.0);

    context.new_sub_path();
    context.move_to(x, y);
    context.line_to(x - tx, y + (dy - h));
    context.arc(x, y - h, radius, dy.atan2(-tx), dy.atan2(tx));
    context.line_to(x, y);
    context.close_path();
    context.fill()?;

    Ok(h + radius)
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

        geom.transform(&proj).expect("geometry transformed");

        let geom = geom.project_to_tile(&ctx.tile_projector);

        let PointProps { marker_svg, .. } = parse_point_props(feature);

        walk_geometry_points(&geom, &mut |point| -> cairo::Result<()> {
            let x = point.x();
            let y = point.y();

            // The marker SVG is fully self-contained and authored so its anchor
            // (the geographic location) is the exact center of the viewBox, so
            // every shape uses the same rule: center the bitmap on (x, y).
            if let Some(svg) = marker_svg.as_deref()
                && let Some(height) = render_marker_svg(context, svg, x, y)
            {
                // Register the marker footprint so other layers respect it.
                collision.add(Rect::new(
                    (x - MARKER_WIDTH / 2.0, y - height / 2.0),
                    (x + MARKER_WIDTH / 2.0, y + height / 2.0),
                ));

                return Ok(());
            }

            // Fall back to a default marker (tip on the point).
            let h = draw_default_marker(context, x, y)?;

            collision.add(Rect::new(
                (x - MARKER_WIDTH / 2.0, y - h),
                (x + MARKER_WIDTH / 2.0, y),
            ));

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

        geom.transform(&proj).expect("geometry transformed");

        let geom = geom.project_to_tile(&ctx.tile_projector);

        if matches!(geom, Geometry::Point(_)) {
            continue;
        }

        let Some(name) = parse_title(feature) else {
            continue;
        };

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

        geom.transform(&proj).expect("geometry transformed");

        let geom = geom.project_to_tile(&ctx.tile_projector);

        let Geometry::Point(point) = geom else {
            continue;
        };

        let PointProps { name, marker_svg } = parse_point_props(feature);

        let Some(name) = name else { continue };

        // Place the label just above the top of the marker. The marker's anchor
        // is the viewBox center, so its top edge is half its rendered height
        // above the point; the default marker is ~64 px tall (tip on the point).
        let half_height = marker_svg
            .as_deref()
            .and_then(marker_svg_height)
            .map_or(32.0, |h| h / 2.0);

        let point = point.translate(0.0, -(half_height + 12.0));

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
