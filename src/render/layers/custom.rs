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
    render_request::LabelStyle,
};
use cairo::{Context, LineCap, LineJoin, Rectangle};
use colorsys::{Rgb, RgbRatio};
use cosmic_text::Weight;
use geo::{Geometry, InteriorPoint, Rect, Transform};
use geojson::Feature;
use gio::glib;
use proj::Proj;
use serde_json::Value;

// The rendered marker width (`marker_width`) and the per-side glow width
// (`glow_width`) are supplied per request (see `CustomLayer`/`Glow`) and threaded
// through the functions below. All marker SVGs share viewBox width 310 and are
// scaled to `marker_width`; height follows from each SVG's aspect ratio. The
// render context is already scaled by the request's `scale`, so no extra scaling
// is needed here.

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

/// Draw the shared glow layer for the whole custom overlay: a halo behind every
/// line, polygon edge and marker. All halos are rendered into one group at the
/// solid (opaque) glow `color` — so overlaps union instead of stacking — then
/// composited once at the color's alpha. The colored strokes
/// ([`render_lines_polygons`]) and the markers ([`render_points`]) are painted
/// on top afterwards, leaving only the outer halo visible.
///
/// `color` is `(r, g, b, a)` in 0.0..=1.0; `a` is the glow opacity. `glow_width`
/// is the per-side halo width and `marker_width` the rendered marker width (both
/// tile/CSS px). Must run before `render_lines_polygons`/`render_points` in the
/// pipeline.
pub fn render_glow(
    ctx: &Ctx,
    context: &Context,
    features: &[Feature],
    color: (f64, f64, f64, f64),
    marker_width: f64,
    glow_width: f64,
) -> LayerRenderResult {
    let proj = make_proj();
    let (r, g, b, a) = color;
    let hex = rgb_hex(r, g, b);

    context.push_group();

    // Line and polygon-edge halos: each stroked `glow_width` wider on every side.
    // Solid even under dashed lines, so the glow reads as a continuous outline
    // rather than a dashed shadow.
    for feature in features {
        let mut geom: Geometry = Geometry::try_from(feature.clone())?;
        geom.transform(&proj).expect("geometry transformed");
        let geom = geom.project_to_tile(&ctx.tile_projector);

        let props = parse_line_polygon_props(feature);

        path_geometry(context, &geom);
        context.set_line_width(2.0f64.mul_add(glow_width, props.width));
        context.set_source_rgb(r, g, b);
        context.set_line_join(props.line_join.unwrap_or(LineJoin::Round));
        context.set_line_cap(props.line_cap.unwrap_or(LineCap::Round));
        context.set_dash(&[], 0.0);
        context.stroke()?;
    }

    // Marker halos: a dilated, recolored silhouette of each marker.
    for feature in features {
        let mut geom: Geometry = Geometry::try_from(feature.clone())?;
        geom.transform(&proj).expect("geometry transformed");
        let geom = geom.project_to_tile(&ctx.tile_projector);

        let PointProps { marker_svg, .. } = parse_point_props(feature);

        walk_geometry_points(&geom, &mut |point| -> cairo::Result<()> {
            let x = point.x();
            let y = point.y();

            if let Some(svg) = marker_svg.as_deref()
                && render_marker_glow(context, svg, (x, y), &hex, marker_width, glow_width)
                    .is_some()
            {
                return Ok(());
            }

            draw_default_marker_glow(context, (x, y), (r, g, b), glow_width)
        })?;
    }

    context.pop_group_to_source()?;
    context.paint_with_alpha(a)?;

    Ok(())
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

    // Pass 2: strokes (polygon borders and lines). The white glow halo beneath
    // them is drawn earlier by `render_glow`, as a single shared layer.
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
fn render_marker_svg(
    context: &Context,
    svg: &str,
    (x, y): (f64, f64),
    marker_width: f64,
) -> Option<f64> {
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

    let height = marker_width * sh / sw;

    let left = x - marker_width / 2.0;
    let top = y - height / 2.0;

    renderer
        .render_document(context, &Rectangle::new(left, top, marker_width, height))
        .ok()?;

    Some(height)
}

/// Render a marker SVG recolored to a solid glow-color silhouette (`hex`) and
/// dilated outward by `glow_width` (via a wide same-color stroke), positioned
/// like [`render_marker_svg`]. Used by [`render_glow`] to build the marker halo;
/// the real marker is later painted on top, leaving only the dilated ring
/// visible.
fn render_marker_glow(
    context: &Context,
    svg: &str,
    (x, y): (f64, f64),
    hex: &str,
    marker_width: f64,
    glow_width: f64,
) -> Option<()> {
    let bytes = glib::Bytes::from_owned(svg.as_bytes().to_vec());

    let stream = gio::MemoryInputStream::from_bytes(&bytes);

    let mut handle = rsvg::Loader::new()
        .read_stream(&stream, None::<&gio::File>, None::<&gio::Cancellable>)
        .ok()?;

    let (sw, sh) = rsvg::CairoRenderer::new(&handle).intrinsic_size_in_pixels()?;

    if sw <= 0.0 || sh <= 0.0 {
        return None;
    }

    // Recolor every shape to a solid glow-color silhouette and add a same-color
    // stroke to dilate it. The stroke width is in the SVG's own user units, so
    // scale it so the dilation equals `glow_width` tile px once scaled down to
    // `marker_width`. Force full opacity so the silhouette is solid regardless of
    // the marker's own `*-opacity` (e.g. a ring drawn at `stroke-opacity`), which
    // would otherwise make the glow weaker than the line halo.
    let stroke_width = 2.0 * glow_width * sw / marker_width;
    let css = format!(
        "* {{ fill: {hex}; stroke: {hex}; stroke-width: {stroke_width}px; \
         stroke-linejoin: round; opacity: 1; fill-opacity: 1; stroke-opacity: 1; }}"
    );
    handle.set_stylesheet(&css).ok()?;

    let renderer = rsvg::CairoRenderer::new(&handle);

    let height = marker_width * sh / sw;

    let left = x - marker_width / 2.0;
    let top = y - height / 2.0;

    renderer
        .render_document(context, &Rectangle::new(left, top, marker_width, height))
        .ok()?;

    Some(())
}

/// An `(r, g, b)` color (components in 0.0..=1.0) as a `#rrggbb` hex string for
/// use in rsvg user stylesheets.
fn rgb_hex(r: f64, g: f64, b: f64) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "color components are in 0.0..=1.0"
    )]
    let to_u8 = |c: f64| (c * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", to_u8(r), to_u8(g), to_u8(b))
}

/// Rendered height of a `marker-svg` (scaled to `marker_width`), derived from
/// the root element's `width`/`height` attributes. Used to position the label
/// above the marker without re-rasterizing.
fn marker_svg_height(svg: &str, marker_width: f64) -> Option<f64> {
    let el = xmltree::Element::parse(svg.as_bytes()).ok()?;

    let w: f64 = el.attributes.get("width")?.parse().ok()?;
    let h: f64 = el.attributes.get("height")?.parse().ok()?;

    if w <= 0.0 {
        return None;
    }

    Some(marker_width * h / w)
}

/// Append the default-marker teardrop path (tip on `(x, y)`) to the current
/// path and return its rendered height (tip to top). Shared by the filled
/// marker and its glow outline.
fn default_marker_path(context: &Context, (x, y): (f64, f64)) -> f64 {
    let radius = 10f64;
    let h = radius * 2.2;
    let dy = radius * radius / h;
    let tx = radius.mul_add(radius, -(dy * dy)).max(0.0).sqrt();

    context.new_sub_path();
    context.move_to(x, y);
    context.line_to(x - tx, y + (dy - h));
    context.arc(x, y - h, radius, dy.atan2(-tx), dy.atan2(tx));
    context.line_to(x, y);
    context.close_path();

    h + radius
}

/// Draw the default marker (a teardrop pin) for point features that carry no
/// `marker-svg` — e.g. points imported from tracks or search. Its tip sits on
/// `(x, y)`. Point features have no color of their own, so it uses the app's
/// default marker color (`#d00000`).
fn draw_default_marker(context: &Context, (x, y): (f64, f64)) -> cairo::Result<f64> {
    context.set_source_rgb(0.815_686, 0.0, 0.0);

    let h = default_marker_path(context, (x, y));
    context.fill()?;

    Ok(h)
}

/// Draw the default marker's glow outline: the same teardrop filled and stroked
/// in the glow color `(r, g, b)`, dilating it outward by `GLOW_WIDTH_EXTRA`. The
/// real marker is painted on top later, leaving only the dilated ring visible.
fn draw_default_marker_glow(
    context: &Context,
    (x, y): (f64, f64),
    (r, g, b): (f64, f64, f64),
    glow_width: f64,
) -> cairo::Result<()> {
    context.set_source_rgb(r, g, b);

    default_marker_path(context, (x, y));

    context.set_line_width(2.0 * glow_width);
    context.set_line_join(LineJoin::Round);
    context.fill_preserve()?;
    context.stroke()?;

    Ok(())
}

pub fn render_points(
    ctx: &Ctx,
    context: &Context,
    features: &[Feature],
    collision: &mut Collision,
    marker_width: f64,
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
                && let Some(height) = render_marker_svg(context, svg, (x, y), marker_width)
            {
                // Register the marker footprint so other layers respect it.
                collision.add(Rect::new(
                    (x - marker_width / 2.0, y - height / 2.0),
                    (x + marker_width / 2.0, y + height / 2.0),
                ));

                return Ok(());
            }

            // Fall back to a default marker (tip on the point).
            let h = draw_default_marker(context, (x, y))?;

            collision.add(Rect::new(
                (x - marker_width / 2.0, y - h),
                (x + marker_width / 2.0, y),
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
    label_style: LabelStyle,
) -> LayerRenderResult {
    let proj = make_proj();

    let size = label_style.size.unwrap_or(15.0);
    let weight = label_style.weight.unwrap_or_default();

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
            let mut options = TextOnLineOptions {
                flo: FontAndLayoutOptions {
                    size,
                    weight,
                    ..Default::default()
                },
                halo_width: 2.0,
                ..Default::default()
            };

            if let Some(color) = label_style.color {
                options.color = color;
            }

            walk_geometry_line_strings(&geom, &mut |ls| {
                let _ = draw_text_on_line(context, ls, &name, Some(collision), &options)?;
                cairo::Result::Ok(())
            })?;
        } else {
            let Some(point) = geom.interior_point() else {
                continue;
            };

            let mut options = TextOptions {
                flo: FontAndLayoutOptions {
                    size,
                    weight,
                    ..Default::default()
                },
                halo_width: 2.0,
                ..Default::default()
            };

            if let Some(color) = label_style.color {
                options.color = color;
            }

            let _ = draw_text(context, Some(collision), &point, &name, &options);
        }
    }

    Ok(())
}

pub fn render_point_labels(
    ctx: &Ctx,
    context: &Context,
    features: &[Feature],
    collision: &mut Collision,
    marker_width: f64,
    label_style: LabelStyle,
) -> LayerRenderResult {
    let proj = make_proj();

    let size = label_style.size.unwrap_or(15.0);
    let weight = label_style.weight.unwrap_or(Weight::BOLD);
    let color = label_style.color.unwrap_or((0.0, 0.0, 1.0));

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
            .and_then(|svg| marker_svg_height(svg, marker_width))
            .map_or(32.0, |h| h / 2.0);

        // Anchor the label by its near edge (`valign_by_placement`) instead of
        // its center, so a multi-line label stacks away from the marker rather
        // than growing into it (which would collide with the marker footprint
        // and suppress the whole label). Prefer placing it above the marker;
        // fall back to below it when the above placements collide.
        let placements = [
            (0.0, -half_height - 4.0),
            (0.0, -half_height - 6.0),
            (0.0, -half_height - 8.0),
            (0.0, half_height),
            (0.0, half_height + 2.0),
            (0.0, half_height + 4.0),
        ];

        let _ = draw_text(
            context,
            Some(collision),
            &point,
            &name,
            &TextOptions {
                flo: FontAndLayoutOptions {
                    size,
                    weight,
                    ..Default::default()
                },
                color,
                halo_width: 2.0,
                valign_by_placement: true,
                placements: &placements,
                ..Default::default()
            },
        );
    }

    Ok(())
}
