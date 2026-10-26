use crate::render::{
    colors::{self, Color, ContextExt, rgb_hex},
    draw::{
        font_options::{FontAndLayoutOptions, Weight},
        font_system::stamp_outline,
        path_geom::path_polygons,
        text_on_line::{InlineImage, shaped_clusters},
    },
    svg_repo::SvgRepo,
};
use cairo::Context;
use geo::{
    BooleanOps, Buffer as _, Coord, Geometry, LineString, MultiPolygon, Polygon,
    algorithm::{
        bool_ops::FillRule,
        buffer::{BufferStyle, LineCap, LineJoin},
    },
};
use rustc_hash::FxHashMap;
use std::{
    cell::RefCell,
    f64::consts::{PI, TAU},
    rc::Rc,
};

/// A foreground's colour and shape name.
type Shape = (Color, String);

/// Parsed `osmc:symbol`: `waycolor:background[:foreground[:foreground2]][:text:textcolor]`.
struct Symbol {
    background: Option<(Color, Background)>,
    foregrounds: Vec<Shape>,
    text: Option<(String, Color)>,
}

/// Width and height of a symbol, in pixels.
pub const SYMBOL_SIZE: f64 = 9.0;

/// Kinds of background, all drawn within the same square.
#[derive(Clone, Copy)]
enum Background {
    Square,
    /// A disc.
    Round,
    /// A ring around a white disc.
    Circle,
    /// A border around a white square.
    Frame,
}

const RED: Color = colors::parse_color("#e00000");
const BLUE: Color = colors::parse_color("#0040e0");
const GREEN: Color = colors::parse_color("#00a000");
// Pure yellow fades into the white background.
const YELLOW: Color = colors::parse_color("#e0b000");
const ORANGE: Color = colors::parse_color("#ff8000");
const PURPLE: Color = colors::parse_color("#a000c0");
const BROWN: Color = colors::parse_color("#8b4513");
const GRAY: Color = colors::parse_color("#808080");
const PINK: Color = colors::parse_color("#ff60c0");
const CYAN: Color = colors::parse_color("#00b0e0");

fn color(name: &str) -> Option<Color> {
    Some(match name {
        "black" => colors::BLACK,
        "white" => colors::WHITE,
        "red" => RED,
        "blue" => BLUE,
        "green" => GREEN,
        "yellow" => YELLOW,
        "orange" => ORANGE,
        "purple" | "violet" => PURPLE,
        "brown" => BROWN,
        "gray" | "grey" => GRAY,
        "pink" => PINK,
        "cyan" => CYAN,
        _ => return None,
    })
}

/// `red_bar` → (red, bar); bare names such as `shell_modern` keep their own colours.
fn foreground(value: &str) -> Option<Shape> {
    if let Some((name, shape)) = value.split_once('_')
        && let Some(c) = color(name)
    {
        return (shape != "none").then(|| (c, shape.to_owned()));
    }

    match value {
        "shell" | "shell_modern" => Some((YELLOW, value.to_owned())),
        // A colour alone has no shape; mistagged text colours end up here.
        "" | "none" => None,
        _ if color(value).is_some() => None,
        _ => Some((colors::BLACK, value.to_owned())),
    }
}

/// The background, or the foreground it holds instead.
fn background(bg: &str) -> (Option<(Color, Background)>, Option<Shape>) {
    let (name, kind) = bg.split_once('_').unwrap_or((bg, ""));

    let kind = match kind {
        "" => Background::Square,
        "round" => Background::Round,
        "circle" => Background::Circle,
        "frame" => Background::Frame,
        // A shape in place of the background, as in `orange:orange_arrow`, is a foreground.
        _ => return (None, foreground(bg)),
    };

    (color(name).map(|c| (c, kind)), None)
}

fn parse(value: &str) -> Option<Symbol> {
    let parts: Vec<&str> = value.split(':').map(str::trim).collect();

    let (background, background_shape) = parts.get(1).map_or((None, None), |bg| background(bg));

    let rest = parts.get(2..).unwrap_or_default();

    // A text needs its colour after it; `waycolor:bg:text:textcolor` omits the foreground.
    let text_at_end = match rest {
        [fgs @ .., text, text_color]
            if !text.is_empty() && (!fgs.is_empty() || !text.contains('_')) =>
        {
            color(text_color).map(|c| (fgs, ((*text).to_owned(), c)))
        }
        // A text without its colour, as in `red:white::EB`; shape names are longer words.
        [fgs @ .., text]
            if (1..=3).contains(&text.chars().count())
                && !text.contains('_')
                && color(text).is_none() =>
        {
            Some((fgs, ((*text).to_owned(), colors::BLACK)))
        }
        _ => None,
    };

    let (fgs, text) = text_at_end.map_or((rest, None), |(fgs, text)| (fgs, Some(text)));

    let foregrounds: Vec<Shape> = background_shape
        .into_iter()
        .chain(fgs.iter().take(2).filter_map(|fg| foreground(fg)))
        .collect();

    // An empty ring, as in `red:red_circle`, stands for a dot of its colour.
    let background = match background {
        Some((c, Background::Circle)) if foregrounds.is_empty() && text.is_none() => {
            Some((c, Background::Round))
        }
        other => other,
    };

    let symbol = Symbol {
        background,
        foregrounds,
        text,
    };

    (symbol.background.is_some() || !symbol.foregrounds.is_empty() || symbol.text.is_some())
        .then_some(symbol)
}

fn path_points(cr: &Context, pts: impl IntoIterator<Item = (f64, f64)>) {
    for (i, (x, y)) in pts.into_iter().enumerate() {
        if i == 0 {
            cr.move_to(x, y);
        } else {
            cr.line_to(x, y);
        }
    }

    cr.close_path();
}

type Segment = ((f64, f64), (f64, f64));

/// Where shapes go instead of being painted, as polygons in the unit square: they make the
/// outline of a symbol without a background.
type Collect<'a> = Option<&'a RefCell<Vec<MultiPolygon>>>;

/// Device units per unit of the square while collecting; sets how finely curves are flattened.
const COLLECT_SCALE: f64 = 100.0;

/// The current path, flattened, as rings in the unit square with whether each is closed.
fn flat_rings(cr: &Context) -> cairo::Result<Vec<(Vec<Coord>, bool)>> {
    use cairo::PathSegment;

    let mut rings: Vec<(Vec<Coord>, bool)> = Vec::new();

    for segment in cr.copy_path_flat()?.iter() {
        match segment {
            PathSegment::MoveTo((x, y)) | PathSegment::LineTo((x, y)) => {
                let (x, y) = cr.user_to_device(x, y);
                let coord = Coord {
                    x: x / COLLECT_SCALE,
                    y: y / COLLECT_SCALE,
                };

                match (segment, rings.last_mut()) {
                    (PathSegment::LineTo(_), Some((ring, false))) => ring.push(coord),
                    _ => rings.push((vec![coord], false)),
                }
            }
            PathSegment::ClosePath => {
                if let Some((_, closed)) = rings.last_mut() {
                    *closed = true;
                }
            }
            PathSegment::CurveTo(..) => {}
        }
    }

    Ok(rings)
}

/// The area the current path fills, by cairo's default non-zero rule.
fn collect_fill(cr: &Context, shapes: &RefCell<Vec<MultiPolygon>>) -> cairo::Result<()> {
    let rings = MultiPolygon::new(
        flat_rings(cr)?
            .into_iter()
            .filter(|(ring, _)| ring.len() > 2)
            .map(|(ring, _)| Polygon::new(LineString::new(ring), Vec::new()))
            .collect(),
    );

    shapes
        .borrow_mut()
        .push(rings.union_with_fill_rule(&MultiPolygon::new(Vec::new()), FillRule::NonZero));

    Ok(())
}

/// The area stroking the current path covers, with its width, cap and join.
fn collect_stroke(cr: &Context, shapes: &RefCell<Vec<MultiPolygon>>) -> cairo::Result<()> {
    let (dx, dy) = cr.user_to_device_distance(cr.line_width(), 0.0)?;
    let half_width = dx.hypot(dy) / COLLECT_SCALE / 2.0;

    let cap = match cr.line_cap() {
        cairo::LineCap::Round => LineCap::Round(0.2),
        cairo::LineCap::Square => LineCap::Square,
        _ => LineCap::Butt,
    };

    let join = match cr.line_join() {
        cairo::LineJoin::Round => LineJoin::Round(0.2),
        cairo::LineJoin::Bevel => LineJoin::Bevel,
        _ => LineJoin::Miter(0.1),
    };

    let style = BufferStyle::new(half_width)
        .line_cap(cap)
        .line_join(join.clone());
    let mut shapes = shapes.borrow_mut();

    for (ring, closed) in flat_rings(cr)? {
        let meets = |a: &Coord, b: &Coord| (a.x - b.x).hypot(a.y - b.y) < 1e-6;

        // A line ending where it starts, such as an arc drawn round, is a loop: its band is the
        // shape grown minus the shape shrunk, as `buffer` would give a line butt ends and a notch.
        if ring.len() > 2
            && (closed
                || ring
                    .first()
                    .zip(ring.last())
                    .is_some_and(|(a, b)| meets(a, b)))
        {
            let polygon = Polygon::new(LineString::new(ring), Vec::new());
            let grown =
                polygon.buffer_with_style(BufferStyle::new(half_width).line_join(join.clone()));
            let shrunk =
                polygon.buffer_with_style(BufferStyle::new(-half_width).line_join(join.clone()));

            shapes.push(grown.difference(&shrunk));
        } else if ring.len() > 1 {
            shapes.push(LineString::new(ring).buffer_with_style(style.clone()));
        }
    }

    Ok(())
}

fn fill(cr: &Context, c: Color, collect: Collect) -> cairo::Result<()> {
    if let Some(shapes) = collect {
        collect_fill(cr, shapes)?;
        cr.new_path();
        return Ok(());
    }

    cr.set_source_color(c);
    cr.fill()
}

fn stroke(cr: &Context, c: Color, collect: Collect) -> cairo::Result<()> {
    if let Some(shapes) = collect {
        collect_stroke(cr, shapes)?;
        cr.new_path();
        return Ok(());
    }

    cr.set_source_color(c);
    cr.stroke()
}

fn stroke_lines(
    cr: &Context,
    c: Color,
    collect: Collect,
    width: f64,
    lines: impl IntoIterator<Item = Segment>,
) -> cairo::Result<()> {
    for ((x1, y1), (x2, y2)) in lines {
        cr.move_to(x1, y1);
        cr.line_to(x2, y2);
    }

    cr.set_line_width(width);
    stroke(cr, c, collect)
}

/// A water drop, pointing up.
fn path_drop(cr: &Context) {
    // The tip's lines touch the round part 60° either side of its top.
    cr.move_to(0.5, 0.12);
    cr.arc(0.5, 0.62, 0.25, -PI / 6.0, 7.0 * PI / 6.0);
    cr.close_path();
}

/// Largest share of the square a shape takes, unless it reaches the edges by design.
const SHAPE_FIT: f64 = 0.7;

/// Shapes that reach the edges of the square by design.
const EDGE_TO_EDGE: [&str; 11] = [
    "bar",
    "stripe",
    "upper",
    "lower",
    "left",
    "right",
    "corner",
    "corner_left",
    "backslash",
    "slash",
    "frame",
];

/// Paints a foreground shape centred in the unit square and shrunk to [`SHAPE_FIT`] of it, so
/// that all shapes keep alike margins; unknown shapes get a `?`.
fn paint_foreground(cr: &Context, c: Color, shape: &str, collect: Collect) -> cairo::Result<()> {
    if EDGE_TO_EDGE.contains(&shape) {
        return paint_shape(cr, c, shape, collect);
    }

    // Halves are placed by their whole, so that they meet it and each other.
    let whole = match shape {
        "diamond_left" | "diamond_right" => "diamond",
        _ => shape,
    };

    let Some((x, y, w, h)) = shape_extents(whole) else {
        return paint_shape(cr, c, shape, collect);
    };

    let scale = (SHAPE_FIT / w.max(h)).min(1.0);

    cr.save()?;
    cr.translate(0.5, 0.5);
    cr.scale(scale, scale);
    cr.translate(-w.mul_add(0.5, x), -h.mul_add(0.5, y));
    paint_shape(cr, c, shape, collect)?;
    cr.restore()
}

/// Ink box of a shape in the unit square, as `(x, y, width, height)`.
fn shape_extents(shape: &str) -> Option<(f64, f64, f64, f64)> {
    const SCALE: f64 = 100.0;

    let surface = cairo::RecordingSurface::create(cairo::Content::ColorAlpha, None).ok()?;
    let cr = Context::new(&surface).ok()?;
    cr.scale(SCALE, SCALE);
    paint_shape(&cr, colors::BLACK, shape, None).ok()?;
    drop(cr);

    let (x, y, w, h) = surface.ink_extents();

    (w > 0.0 && h > 0.0).then_some((x / SCALE, y / SCALE, w / SCALE, h / SCALE))
}

/// [`paint_foreground`] at the shape's own size.
fn paint_shape(cr: &Context, c: Color, shape: &str, collect: Collect) -> cairo::Result<()> {
    const T: f64 = 1.0 / 3.0;

    // Within the unit square: left to right, and a right-pointing shape turned to point up.
    let mirror = cairo::Matrix::new(-1.0, 0.0, 0.0, 1.0, 1.0, 0.0);
    let turn_up = cairo::Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, 1.0);

    // Other names in use for the shapes below, some mirrored or turned.
    let alias = match shape {
        "square" => Some(("rectangle", None)),
        "trapezium" => Some(("trapezoid", None)),
        "right_pointer" => Some(("pointer", None)),
        "right_arrow" => Some(("arrow", None)),
        "left_pointer" => Some(("pointer", Some(mirror))),
        "left_arrow" => Some(("arrow", Some(mirror))),
        "up_arrow" => Some(("arrow", Some(turn_up))),
        _ => None,
    };

    if let Some((shape, matrix)) = alias {
        cr.save()?;

        if let Some(matrix) = matrix {
            cr.transform(matrix);
        }

        paint_foreground(cr, c, shape, collect)?;

        return cr.restore();
    }

    // Where a diagonal band `T` wide meets the square's edge.
    const DIAGONAL: f64 = T * std::f64::consts::FRAC_1_SQRT_2;

    match shape {
        "bar" => cr.rectangle(0.0, T, 1.0, T),
        "stripe" => cr.rectangle(T, 0.0, T, 1.0),
        "lower" => cr.rectangle(0.0, 0.5, 1.0, 0.5),
        "upper" => cr.rectangle(0.0, 0.0, 1.0, 0.5),
        "left" => cr.rectangle(0.0, 0.0, 0.5, 1.0),
        "right" => cr.rectangle(0.5, 0.0, 0.5, 1.0),
        "rectangle" => cr.rectangle(0.25, 0.25, 0.5, 0.5),
        "corner" => path_points(cr, [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        "corner_left" => path_points(cr, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]),
        "drop" => path_drop(cr),
        "shield" | "crest" => {
            cr.move_to(0.2, 0.15);
            cr.line_to(0.8, 0.15);
            cr.line_to(0.8, 0.45);
            cr.curve_to(0.8, 0.7, 0.6, 0.82, 0.5, 0.88);
            cr.curve_to(0.4, 0.82, 0.2, 0.7, 0.2, 0.45);
            cr.close_path();
        }
        "heart" => {
            cr.move_to(0.5, 0.86);
            cr.arc(0.35, 0.4, 0.2, 0.75 * PI, 2.0 * PI);
            cr.arc(0.65, 0.4, 0.2, PI, 2.25 * PI);
            cr.close_path();
        }
        "triangle" => path_points(cr, [(0.5, 0.15), (0.88, 0.82), (0.12, 0.82)]),
        "triangle_turned" => path_points(cr, [(0.12, 0.18), (0.88, 0.18), (0.5, 0.85)]),
        "diamond" => path_points(cr, [(0.5, 0.1), (0.9, 0.5), (0.5, 0.9), (0.1, 0.5)]),
        "diamond_left" => path_points(cr, [(0.5, 0.1), (0.5, 0.9), (0.1, 0.5)]),
        "diamond_right" => path_points(cr, [(0.5, 0.1), (0.9, 0.5), (0.5, 0.9)]),
        // `black_red_diamond`: a diamond half in the given colour, half red.
        "red_diamond" => {
            paint_shape(cr, c, "diamond_left", collect)?;
            return paint_shape(cr, RED, "diamond_right", collect);
        }
        "pointer" => path_points(cr, [(0.15, 0.15), (0.9, 0.5), (0.15, 0.85)]),
        "arrow" => path_points(
            cr,
            [
                (0.1, 0.42),
                (0.55, 0.42),
                (0.55, 0.2),
                (0.92, 0.5),
                (0.55, 0.8),
                (0.55, 0.58),
                (0.1, 0.58),
            ],
        ),
        "house" => path_points(
            cr,
            [
                (0.2, 0.85),
                (0.2, 0.45),
                (0.5, 0.15),
                (0.8, 0.45),
                (0.8, 0.85),
            ],
        ),
        "trapezoid" => path_points(cr, [(0.3, 0.25), (0.7, 0.25), (0.88, 0.75), (0.12, 0.75)]),
        "tower" => {
            cr.rectangle(0.25, 0.15, 0.5, 0.2);
            cr.rectangle(0.37, 0.35, 0.26, 0.5);
        }
        "hexagon" => path_points(
            cr,
            (0..6).map(|i| {
                let a = f64::from(i) * PI / 3.0;
                (0.4f64.mul_add(a.cos(), 0.5), 0.4f64.mul_add(a.sin(), 0.5))
            }),
        ),
        "dot" => cr.arc(0.5, 0.5, 0.3, 0.0, TAU),
        "bowl" => {
            cr.arc(0.5, 0.3, 0.35, 0.0, PI);
            cr.close_path();
        }
        "turned_T" => {
            cr.rectangle(0.2, 0.68, 0.6, 0.17);
            cr.rectangle(0.415, 0.15, 0.17, 0.6);
        }
        "L" => {
            cr.rectangle(0.2, 0.15, 0.17, 0.7);
            cr.rectangle(0.2, 0.68, 0.6, 0.17);
        }
        "cross" => {
            cr.rectangle(0.1, 0.415, 0.8, 0.17);
            cr.rectangle(0.415, 0.1, 0.17, 0.8);
        }
        // Diagonal bands of width `T`, cut by the square.
        "backslash" => path_points(
            cr,
            [
                (0.0, 0.0),
                (DIAGONAL, 0.0),
                (1.0, 1.0 - DIAGONAL),
                (1.0, 1.0),
                (1.0 - DIAGONAL, 1.0),
                (0.0, DIAGONAL),
            ],
        ),
        "slash" => path_points(
            cr,
            [
                (0.0, 1.0),
                (0.0, 1.0 - DIAGONAL),
                (1.0 - DIAGONAL, 0.0),
                (1.0, 0.0),
                (1.0, DIAGONAL),
                (DIAGONAL, 1.0),
            ],
        ),
        "diamond_line" => {
            path_points(cr, [(0.5, 0.15), (0.85, 0.5), (0.5, 0.85), (0.15, 0.5)]);
            cr.set_line_width(0.12);
            return stroke(cr, c, collect);
        }
        "drop_line" => {
            path_drop(cr);
            cr.set_line_width(0.1);
            return stroke(cr, c, collect);
        }
        "wheel" => {
            cr.save()?;
            cr.arc(0.5, 0.5, 0.35, 0.0, TAU);
            cr.close_path();
            cr.set_line_width(0.1);
            stroke(cr, c, collect)?;

            for i in 0..6 {
                let a = f64::from(i) * PI / 3.0;
                cr.move_to(0.5, 0.5);
                cr.line_to(0.35f64.mul_add(a.cos(), 0.5), 0.35f64.mul_add(a.sin(), 0.5));
            }

            cr.set_line_width(0.06);
            stroke(cr, c, collect)?;
            cr.arc(0.5, 0.5, 0.08, 0.0, TAU);
            fill(cr, c, collect)?;

            return cr.restore();
        }
        "fork" => {
            cr.move_to(0.5, 0.9);
            cr.line_to(0.5, 0.5);
            cr.line_to(0.18, 0.12);
            cr.move_to(0.5, 0.5);
            cr.line_to(0.82, 0.12);
            cr.set_line_width(0.15);
            return stroke(cr, c, collect);
        }
        "hiker" => {
            cr.save()?;
            cr.arc(0.55, 0.14, 0.12, 0.0, TAU);
            fill(cr, c, collect)?;
            // Torso, legs, arms and stick.
            cr.move_to(0.52, 0.3);
            cr.line_to(0.45, 0.58);
            cr.line_to(0.3, 0.9);
            cr.move_to(0.45, 0.58);
            cr.line_to(0.62, 0.9);
            cr.move_to(0.3, 0.5);
            cr.line_to(0.5, 0.35);
            cr.line_to(0.72, 0.5);
            cr.move_to(0.76, 0.35);
            cr.line_to(0.76, 0.92);
            cr.set_line_width(0.14);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_line_join(cairo::LineJoin::Round);
            stroke(cr, c, collect)?;
            return cr.restore();
        }
        "turned_V" => {
            cr.move_to(0.15, 0.85);
            cr.line_to(0.5, 0.15);
            cr.line_to(0.85, 0.85);
            cr.set_line_width(0.17);
            return stroke(cr, c, collect);
        }
        "x" => {
            return stroke_lines(
                cr,
                c,
                collect,
                0.17,
                [((0.15, 0.15), (0.85, 0.85)), ((0.85, 0.15), (0.15, 0.85))],
            );
        }
        "circle" => {
            cr.arc(0.5, 0.5, 0.3, 0.0, TAU);
            cr.set_line_width(0.15);
            return stroke(cr, c, collect);
        }
        // Not a foreground on the wiki, but used as one for a sign with a coloured border.
        "frame" => {
            cr.rectangle(0.075, 0.075, 0.85, 0.85);
            cr.set_line_width(0.15);
            return stroke(cr, c, collect);
        }
        "rectangle_line" => {
            cr.rectangle(0.25, 0.25, 0.5, 0.5);
            cr.set_line_width(0.12);
            return stroke(cr, c, collect);
        }
        "triangle_line" => {
            path_points(cr, [(0.5, 0.18), (0.85, 0.8), (0.15, 0.8)]);
            cr.set_line_width(0.12);
            return stroke(cr, c, collect);
        }
        "arch" => {
            cr.move_to(0.22, 0.85);
            cr.arc(0.5, 0.55, 0.28, PI, 0.0);
            cr.line_to(0.78, 0.85);
            cr.set_line_width(0.17);
            return stroke(cr, c, collect);
        }
        "shell" | "shell_modern" => {
            // Rays fanning out from the left edge.
            let rays = (0..7).map(|i| {
                let a = (f64::from(i) - 3.0) * 0.22;
                (
                    (0.12, 0.5),
                    (0.7f64.mul_add(a.cos(), 0.12), 0.7f64.mul_add(a.sin(), 0.5)),
                )
            });

            return stroke_lines(cr, c, collect, 0.07, rays);
        }
        _ => return paint_unknown(cr, c, collect),
    }

    fill(cr, c, collect)
}

/// A `?` for a shape we do not draw, kept as clear of the edges as the shapes are.
fn paint_unknown(cr: &Context, c: Color, collect: Collect) -> cairo::Result<()> {
    paint_text(cr, "?", c, true, collect)
}

/// Paints `text` centred in the unit square. `large`, it is as tall as the shapes, or less to
/// fit its width, which may exceed theirs so that a ref of a few characters stays readable;
/// otherwise it is small enough to fit in a shape.
fn paint_text(
    cr: &Context,
    text: &str,
    c: Color,
    large: bool,
    collect: Collect,
) -> cairo::Result<()> {
    let flo = FontAndLayoutOptions {
        size: 100.0,
        narrow: text.chars().count() > 1,
        weight: Weight::BOLD,
        ..Default::default()
    };

    let clusters = shaped_clusters(text, &flo);

    let (mut x1, mut y1, mut x2, mut y2) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );

    let mut pen = 0.0;

    for cluster in clusters.iter() {
        x1 = x1.min(pen + cluster.ink_left);
        x2 = x2.max(pen + cluster.ink_right);
        y1 = y1.min(cluster.ink_top);
        y2 = y2.max(cluster.ink_bottom);
        pen += cluster.advance;
    }

    let (w, h) = (x2 - x1, y2 - y1);

    if !(w > 0.0 && h > 0.0) {
        return Ok(());
    }

    let scale = if large {
        (SHAPE_FIT / h).min(0.9 / w)
    } else {
        (0.5 / h).min(0.55 / w)
    };

    cr.save()?;
    cr.new_path();
    cr.translate(0.5, 0.5);
    cr.scale(scale, scale);
    cr.translate(-f64::midpoint(x1, x2), -f64::midpoint(y1, y2));

    let mut pen = 0.0;

    for cluster in clusters.iter() {
        for glyph in &cluster.glyphs {
            if let Some(outline) = &glyph.outline {
                stamp_outline(cr, outline, pen + f64::from(glyph.dx), f64::from(glyph.dy));
            }
        }

        pen += cluster.advance;
    }

    if let Some(shapes) = collect {
        collect_fill(cr, shapes)?;
        cr.new_path();
    } else {
        cr.set_source_color(c);
        cr.fill()?;
    }

    cr.restore()
}

/// Outline of the background, filling the unit square.
fn path_background(cr: &Context, kind: Background) {
    match kind {
        Background::Round | Background::Circle => cr.arc(0.5, 0.5, 0.5, 0.0, TAU),
        Background::Square | Background::Frame => cr.rectangle(0.0, 0.0, 1.0, 1.0),
    }
}

/// `svg_repo` draws the pictograms; without it, they are left out.
fn paint_content(
    cr: &Context,
    symbol: &Symbol,
    collect: Collect,
    svg_repo: Option<&SvgRepo>,
) -> cairo::Result<()> {
    for (c, shape) in &symbol.foregrounds {
        let Some(name) = icon_name(shape) else {
            paint_foreground(cr, *c, shape, collect)?;
            continue;
        };

        // An icon has no paths to collect, so it takes no part in the outline.
        let Some(svg_repo) = svg_repo.filter(|_| collect.is_none()) else {
            continue;
        };

        let stylesheet = format!(
            "path, circle, ellipse, rect, polygon {{ fill: {} }}",
            rgb_hex(*c)
        );

        let margin = (1.0 - SHAPE_FIT) / 2.0;
        let viewport = cairo::Rectangle::new(margin, margin, SHAPE_FIT, SHAPE_FIT);

        if let Err(err) = svg_repo.render_icon(name, &stylesheet, cr, &viewport) {
            eprintln!("osmc:symbol icon: {err}");
            paint_unknown(cr, *c, collect)?;
        }
    }

    if let Some((text, c)) = &symbol.text {
        // A centred shape or a frame is meant to hold the text, which, larger, would spill
        // out of it.
        let within_shape = symbol
            .foregrounds
            .iter()
            .any(|(_, shape)| shape == "frame" || !EDGE_TO_EDGE.contains(&shape.as_str()));

        paint_text(cr, text, *c, !within_shape, collect)?;
    }

    Ok(())
}

/// The map icon for a pictogram shape.
fn icon_name(shape: &str) -> Option<&'static str> {
    Some(match shape {
        "bicycle" | "bike" | "cyclist" => "bicycle",
        "horse" => "horse_riding",
        "mine" => "mine",
        "runner" => "running",
        "chapel" => "chapel",
        _ => return None,
    })
}

/// Union of the symbol's shapes in the unit square.
fn union_outline(symbol: &Symbol) -> Option<MultiPolygon> {
    let surface = cairo::RecordingSurface::create(cairo::Content::ColorAlpha, None).ok()?;
    let cr = Context::new(&surface).ok()?;
    cr.scale(COLLECT_SCALE, COLLECT_SCALE);

    let shapes = RefCell::new(Vec::new());
    paint_content(&cr, symbol, Some(&shapes), None).ok()?;

    shapes.into_inner().into_iter().reduce(|a, b| a.union(&b))
}

fn paint(cr: &Context, symbol: &Symbol, svg_repo: &SvgRepo) -> cairo::Result<()> {
    cr.scale(SYMBOL_SIZE, SYMBOL_SIZE);

    let hairline = 0.6 / SYMBOL_SIZE;

    let Some((background, kind)) = symbol.background else {
        paint_content(cr, symbol, None, Some(svg_repo))?;

        // Without a background, the shapes get the outline instead: of their union, so that
        // shapes touching or overlapping share one.
        if let Some(outline) = union_outline(symbol) {
            path_polygons(cr, &Geometry::MultiPolygon(outline));
            cr.set_source_color_a(colors::BLACK, 0.5);
            cr.set_line_width(hairline);
            cr.set_line_join(cairo::LineJoin::Round);
            cr.stroke()?;
        }

        return Ok(());
    };

    let white_inside = matches!(kind, Background::Frame | Background::Circle);

    cr.save()?;
    path_background(cr, kind);
    cr.set_source_color(if white_inside {
        colors::WHITE
    } else {
        background
    });
    cr.fill_preserve()?;
    cr.clip();

    // Keeps the content clear of a border or a disc's edge.
    let inset = match kind {
        Background::Square => 0.0,
        Background::Round => 0.07,
        Background::Frame | Background::Circle => 0.1,
    };

    cr.translate(inset, inset);
    cr.scale(2.0f64.mul_add(-inset, 1.0), 2.0f64.mul_add(-inset, 1.0));

    paint_content(cr, symbol, None, Some(svg_repo))?;

    cr.restore()?;

    // Sets the symbol apart from the map; a frame or ring is drawn in its own colour.
    match kind {
        Background::Frame => {
            cr.rectangle(0.075, 0.075, 0.85, 0.85);
            cr.set_source_color(background);
            cr.set_line_width(0.15);
        }
        Background::Circle => {
            cr.arc(0.5, 0.5, 0.44, 0.0, TAU);
            cr.set_source_color(background);
            cr.set_line_width(0.12);
            cr.stroke()?;

            // A white ring would not show on its white disc.
            path_background(cr, kind);
            cr.set_source_color_a(colors::BLACK, 0.5);
            cr.set_line_width(hairline);
        }
        Background::Square | Background::Round => {
            path_background(cr, kind);
            cr.set_source_color_a(colors::BLACK, 0.5);
            cr.set_line_width(hairline);
        }
    }

    cr.stroke()
}

/// A symbol recorded once, so that a placement only replays it.
fn record(symbol: &Symbol, svg_repo: &SvgRepo) -> Option<cairo::RecordingSurface> {
    // Outlines centred on the square's edge reach half their width past it.
    const MARGIN: f64 = 1.0;

    let surface = cairo::RecordingSurface::create(
        cairo::Content::ColorAlpha,
        Some(cairo::Rectangle::new(
            -MARGIN,
            -MARGIN,
            2.0f64.mul_add(MARGIN, SYMBOL_SIZE),
            2.0f64.mul_add(MARGIN, SYMBOL_SIZE),
        )),
    )
    .ok()?;

    paint(&Context::new(&surface).ok()?, symbol, svg_repo).ok()?;

    Some(surface)
}

thread_local! {
    static CACHE: RefCell<FxHashMap<String, Option<Rc<InlineImage>>>> =
        RefCell::new(FxHashMap::default());
}

/// Whether an `osmc:symbol` shows `text` itself, ignoring case.
pub fn osmc_symbol_shows(value: &str, text: &str) -> bool {
    !text.is_empty()
        && parse(value)
            .and_then(|symbol| symbol.text)
            .is_some_and(|(shown, _)| shown.eq_ignore_ascii_case(text))
}

/// [`SYMBOL_SIZE`]-wide square image of an `osmc:symbol`, `None` when nothing in it is
/// recognised.
pub fn osmc_symbol_image(value: &str, svg_repo: &SvgRepo) -> Option<Rc<InlineImage>> {
    if value.is_empty() {
        return None;
    }

    if let Some(image) = CACHE.with(|cache| cache.borrow().get(value).cloned()) {
        return image;
    }

    let image = parse(value)
        .and_then(|symbol| record(&symbol, svg_repo))
        .map(|surface| {
            Rc::new(InlineImage {
                width: SYMBOL_SIZE,
                height: SYMBOL_SIZE,
                surface,
            })
        });

    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();

        // Bounded by the distinct symbols in the data, a few hundred.
        if cache.len() >= 4096 {
            cache.clear();
        }

        cache.insert(value.to_owned(), image.clone());
    });

    image
}
