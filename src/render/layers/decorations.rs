use crate::render::{
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        font_system::with_font_system,
        text::{TextOptions, draw_text},
    },
    render_request::Decorations,
};
use cairo::{Context, LineCap, LineJoin};
use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping};
use geo::Point;

/// Inset (in logical pixels) of every decoration from the image edges.
const MARGIN: f64 = 12.0;

/// Draw the requested cartographic decorations on top of the finished map.
///
/// All coordinates are in logical (CSS) pixels: the caller's Cairo context is
/// already scaled by the request's `scale`, and `ctx.size` is the logical size,
/// so the bottom-right corner is `(ctx.size.width, ctx.size.height)`.
pub fn render(ctx: &Ctx, context: &Context, decorations: &Decorations) -> cairo::Result<()> {
    if decorations.scale_bar {
        draw_scale_bar(ctx, context, decorations.center_lat)?;
    }

    if let Some(label) = &decorations.north_arrow {
        draw_north_arrow(ctx, context, label)?;
    }

    if let Some(attribution) = &decorations.attribution {
        draw_attribution(ctx, context, attribution)?;
    }

    Ok(())
}

/// A metric scale bar in the bottom-left corner. The bar length corresponds to a
/// "nice" ground distance (1/2/5 × 10ⁿ) close to a target on-screen width. Units
/// are the universal SI symbols (m/km), so no localization is needed.
fn draw_scale_bar(ctx: &Ctx, context: &Context, center_lat: f64) -> cairo::Result<()> {
    // `meters_per_pixel` is in Web-Mercator metres, which are stretched by
    // 1/cos(latitude); correct to ground metres so the bar reads true distance.
    let ground_mpp = ctx.meters_per_pixel() * center_lat.to_radians().cos();

    if !(ground_mpp.is_finite() && ground_mpp > 0.0) {
        return Ok(());
    }

    const TARGET_PX: f64 = 120.0;

    let nice_dist = nice_distance(ground_mpp * TARGET_PX);
    let bar_px = nice_dist / ground_mpp;

    let (value, unit) = if nice_dist >= 1000.0 {
        (nice_dist / 1000.0, "km")
    } else {
        (nice_dist, "m")
    };

    let label = format!("{} {unit}", format_number(value));

    let x0 = MARGIN;
    let baseline_y = ctx.size.height as f64 - MARGIN;
    let tick_h = 8.0;

    // Staple-shaped path: tick up, across, tick up.
    let path = |context: &Context| {
        context.move_to(x0, baseline_y - tick_h);
        context.line_to(x0, baseline_y);
        context.line_to(x0 + bar_px, baseline_y);
        context.line_to(x0 + bar_px, baseline_y - tick_h);
    };

    context.save()?;
    context.set_line_cap(LineCap::Butt);
    context.set_line_join(LineJoin::Miter);
    context.set_dash(&[], 0.0);

    // White halo underneath, then the black bar on top.
    path(context);
    context.set_source_color_a(colors::WHITE, 0.9);
    context.set_line_width(4.0);
    context.stroke()?;

    path(context);
    context.set_source_color(colors::BLACK);
    context.set_line_width(2.0);
    context.stroke()?;
    context.restore()?;

    draw_text(
        context,
        None,
        // Label centered above the bar.
        &Point::new(x0 + bar_px / 2.0, baseline_y - tick_h - 11.0),
        &label,
        &TextOptions {
            placements: &[(0.0, 0.0)],
            flo: FontAndLayoutOptions {
                size: 13.0,
                ..Default::default()
            },
            halo_width: 2.0,
            ..Default::default()
        },
    )?;

    Ok(())
}

/// A static north arrow in the top-right corner. Exports are always north-up
/// (no bearing), so this is a fixed up-pointing glyph with the (localized) north
/// label beneath it — e.g. "N" (north) or "S" (sever, Slovak).
fn draw_north_arrow(ctx: &Ctx, context: &Context, label: &str) -> cairo::Result<()> {
    let arrow_w = 18.0;
    let arrow_h = 22.0;
    // Depth of the concave notch cut up into the bottom edge.
    let notch = 5.0;
    let mut top = MARGIN;

    let cx = ctx.size.width as f64 - MARGIN - arrow_w / 2.0;

    draw_text(
        context,
        None,
        &Point::new(cx, top + 10.0),
        label,
        &TextOptions {
            placements: &[(0.0, 0.0)],
            flo: FontAndLayoutOptions {
                size: 14.0,
                weight: cosmic_text::Weight::BOLD,
                ..Default::default()
            },
            halo_width: 2.0,
            ..Default::default()
        },
    )?;

    top += 22.0;

    context.save()?;
    context.set_line_join(LineJoin::Round);
    context.set_dash(&[], 0.0);

    // Navigation-style arrowhead: tip at top, two wide base corners, and a
    // concave notch raised up into the middle of the bottom edge.
    context.move_to(cx, top);
    context.line_to(cx + arrow_w / 2.0, top + arrow_h);
    context.line_to(cx, top + arrow_h - notch);
    context.line_to(cx - arrow_w / 2.0, top + arrow_h);
    context.close_path();

    // White halo, then fill black.
    context.set_source_color_a(colors::WHITE, 0.9);
    context.set_line_width(3.0);
    context.stroke_preserve()?;
    context.set_source_color(colors::BLACK);
    context.fill()?;
    context.restore()?;

    Ok(())
}

/// Attribution text, right-aligned in the bottom-right corner.
fn draw_attribution(ctx: &Ctx, context: &Context, attribution: &str) -> cairo::Result<()> {
    const SIZE: f64 = 14.0;

    let width = measure_text_width(attribution, SIZE);

    draw_text(
        context,
        None,
        // `draw_text` centers on the point, so offset the anchor to right-align the
        // text against the right margin and sit it just above the bottom margin.
        &Point::new(
            ctx.size.width as f64 - MARGIN - width / 2.0,
            ctx.size.height as f64 - MARGIN - SIZE / 2.0,
        ),
        attribution,
        &TextOptions {
            placements: &[(0.0, 0.0)],
            flo: FontAndLayoutOptions {
                size: SIZE,
                // Never wrap: attribution is a single line, right-aligned to
                // match the measured width above.
                max_width: f64::INFINITY,
                ..Default::default()
            },
            halo_width: 2.0,
            ..Default::default()
        },
    )?;

    Ok(())
}

/// Round `raw` (in metres) down to a "nice" cartographic value: 1, 2 or 5 times
/// a power of ten.
fn nice_distance(raw: f64) -> f64 {
    let pow = 10f64.powf(raw.log10().floor());
    let frac = raw / pow;

    let nice = if frac >= 5.0 {
        5.0
    } else if frac >= 2.0 {
        2.0
    } else {
        1.0
    };

    nice * pow
}

/// Format a number without a trailing `.0` for whole values.
fn format_number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Lay out `text` on a throwaway buffer to measure its rendered width in logical
/// pixels (the widest line), so callers can right-align it.
fn measure_text_width(text: &str, size: f64) -> f64 {
    with_font_system(|font_system| {
        let metrics = Metrics::new(size as f32, size as f32);
        let mut buffer = Buffer::new(font_system, metrics);
        let attrs = Attrs::new().family(Family::Name("PT Sans"));

        let mut buf = buffer.borrow_with(font_system);
        buf.set_size(Some(f32::INFINITY), None);
        buf.set_text(text, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(true);

        buf.layout_runs()
            .map(|run| run.line_w)
            .fold(0.0f32, f32::max) as f64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_distance_rounds_to_1_2_5() {
        assert_eq!(nice_distance(1.0), 1.0);
        assert_eq!(nice_distance(1.4), 1.0);
        assert_eq!(nice_distance(1.5), 1.0);
        assert_eq!(nice_distance(2.3), 2.0);
        assert_eq!(nice_distance(4.9), 2.0);
        assert_eq!(nice_distance(5.0), 5.0);
        assert_eq!(nice_distance(9.9), 5.0);
        assert_eq!(nice_distance(120.0), 100.0);
        assert_eq!(nice_distance(640.0), 500.0);
        assert_eq!(nice_distance(2300.0), 2000.0);
        assert_eq!(nice_distance(0.7), 0.5);
    }

    #[test]
    fn format_number_strips_trailing_zero() {
        assert_eq!(format_number(500.0), "500");
        assert_eq!(format_number(2.0), "2");
        assert_eq!(format_number(0.5), "0.5");
    }
}
