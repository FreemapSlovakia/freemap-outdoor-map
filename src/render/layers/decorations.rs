use crate::render::{
    attribution::{Attribution, FALLBACK_KEY, OSM as OSM_CODE},
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        font_system::with_font_system,
        text::{TextOptions, draw_text},
    },
    render_request::{AttributionDecoration, Decorations},
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
pub fn render(
    ctx: &Ctx,
    context: &Context,
    decorations: &Decorations,
    attribution: &Attribution,
) -> cairo::Result<()> {
    let scale_bar_right = if decorations.scale_bar {
        draw_scale_bar(ctx, context, decorations.center_lat)?
    } else {
        0.0
    };

    if let Some(label) = &decorations.north_arrow {
        draw_north_arrow(ctx, context, label)?;
    }

    if let Some(decoration) = &decorations.attribution {
        let credits = compose_attribution(decoration, attribution);

        if !credits.is_empty() {
            draw_attribution(ctx, context, &credits, scale_bar_right)?;
        }
    }

    Ok(())
}

/// Where a code sits in the credit line: what every render draws from, then the
/// datasets that answered for somewhere, then the global models that filled what
/// those did not cover. Mirrors the order the web client shows.
fn rank(code: &str) -> u8 {
    if code == OSM_CODE {
        0
    } else if code.ends_with(&format!(":{FALLBACK_KEY}")) {
        2
    } else {
        1
    }
}

/// Code the renderer's own credit is addressed by. Not a dataset — this is the
/// map being drawn — but it takes a `titles` override like one, so a portal
/// serving this map under its own name can say so.
const MAP_CODE: &str = "map";

/// What that credit says when the caller offers nothing.
const FREEMAP: &str = "©\u{a0}Freemap Slovakia";

/// The credits for one render: this map, then the caller's own — whatever it
/// drew that the renderer cannot know — then the datasets that contributed a
/// pixel. A title is named once however many codes carry it, under the
/// strongest claim it has.
fn compose_attribution(decoration: &AttributionDecoration, attribution: &Attribution) -> Vec<String> {
    let mut ranked: Vec<(u8, &str)> = Vec::new();

    for code in attribution.codes() {
        let Some(catalog) = decoration.catalog.get(code) else {
            continue;
        };

        // Only OpenStreetMap's credit is a phrase to translate. Every other
        // title is a rights-holder's own name, and a code can carry several of
        // them — Belgium's relief is two regional models — which one
        // replacement string cannot stand in for without dropping the rest.
        let titles: Vec<&str> = match decoration.overrides.get(code) {
            Some(title) if code == OSM_CODE => vec![title.as_str()],
            _ => catalog.iter().map(String::as_str).collect(),
        };

        for title in titles {
            match ranked.iter_mut().find(|(_, held)| *held == title) {
                Some(held) => held.0 = held.0.min(rank(code)),
                None => ranked.push((rank(code), title)),
            }
        }
    }

    ranked.sort_by_cached_key(|(rank, title)| (*rank, collation_key(title)));

    let mut credits = vec![
        decoration
            .overrides
            .get(MAP_CODE)
            .map_or(FREEMAP, String::as_str)
            .to_owned(),
    ];

    credits.extend(decoration.extra.iter().cloned());

    for (_, title) in ranked {
        if !credits.iter().any(|credit| credit == title) {
            credits.push(title.to_owned());
        }
    }

    credits
}

/// Approximates the `localeCompare` the web client orders the same credits by:
/// case folded, and an accent sorting with its base letter rather than after
/// every unaccented title. A locale that alphabetizes an accented letter in its
/// own right — Slovak sorts `Č` after `C` — still parts company with this.
fn collation_key(title: &str) -> String {
    title
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'ď' | 'đ' => 'd',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
            'ğ' => 'g',
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'į' => 'i',
            'ĺ' | 'ľ' | 'ł' => 'l',
            'ń' | 'ň' | 'ñ' => 'n',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => 'o',
            'ŕ' | 'ř' => 'r',
            'ś' | 'ş' | 'š' => 's',
            'ť' | 'ţ' => 't',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            other => other,
        })
        .collect()
}

/// A metric scale bar in the bottom-left corner. The bar length corresponds to a
/// "nice" ground distance (1/2/5 × 10ⁿ) close to a target on-screen width. Units
/// are the universal SI symbols (m/km), so no localization is needed.
fn draw_scale_bar(ctx: &Ctx, context: &Context, center_lat: f64) -> cairo::Result<f64> {
    // `meters_per_pixel` is in Web-Mercator metres, which are stretched by
    // 1/cos(latitude); correct to ground metres so the bar reads true distance.
    let ground_mpp = ctx.meters_per_pixel() * center_lat.to_radians().cos();

    if !(ground_mpp.is_finite() && ground_mpp > 0.0) {
        return Ok(0.0);
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

    // The label is centered over the bar, so it is what reaches furthest right
    // whenever it is wider than the bar itself.
    let label_w = measure_text_width(&label, 13.0);

    Ok((x0 + bar_px).max(x0 + bar_px / 2.0 + label_w / 2.0))
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

/// Attribution text, right-aligned in the bottom-right corner, wrapped onto as
/// many lines as the image width needs and stacked upwards from the margin.
///
/// `scale_bar_right` is how far the scale bar reaches from the left edge, or
/// `0.0` when none was drawn: it narrows the width the lines wrap to, so they
/// keep clear of the bar they share the bottom of the image with. A single
/// credit wider than what is left still overflows it — no name is broken in
/// half to fit.
fn draw_attribution(
    ctx: &Ctx,
    context: &Context,
    credits: &[String],
    scale_bar_right: f64,
) -> cairo::Result<()> {
    const SIZE: f64 = 14.0;
    const LINE_HEIGHT: f64 = SIZE * 1.25;
    const GAP: f64 = 8.0;

    let left = if scale_bar_right > 0.0 {
        scale_bar_right + GAP
    } else {
        MARGIN
    };

    let available = (ctx.size.width as f64 - MARGIN - left).max(1.0);

    let lines = wrap_attribution(credits, SIZE, available);

    // The block grows upward from the bottom margin, so a caller sending enough
    // credits would paper over the map and run off the top. Keep what fits.
    let fits = ((ctx.size.height as f64 - MARGIN - MARGIN) / LINE_HEIGHT) as usize;
    let lines = &lines[lines.len().saturating_sub(fits.max(1))..];

    for (i, line) in lines.iter().enumerate() {
        // Measured per line, because `draw_text` centers on the point and only an
        // offset of half the line's own width right-aligns it.
        let width = measure_text_width(line, SIZE);

        let from_bottom = (lines.len() - 1 - i) as f64 * LINE_HEIGHT;

        draw_text(
            context,
            None,
            &Point::new(
                ctx.size.width as f64 - MARGIN - width / 2.0,
                ctx.size.height as f64 - MARGIN - SIZE / 2.0 - from_bottom,
            ),
            line,
            &TextOptions {
                placements: &[(0.0, 0.0)],
                flo: FontAndLayoutOptions {
                    size: SIZE,
                    // Wrapped above instead, so each line can be right-aligned
                    // against its own measured width.
                    max_width: f64::INFINITY,
                    ..Default::default()
                },
                halo_width: 2.0,
                ..Default::default()
            },
        )?;
    }

    Ok(())
}

/// Greedily packs the credits into lines no wider than `available`, joining
/// them with ", ". A single credit too wide for that is left to overflow rather
/// than broken mid-name.
fn wrap_attribution(credits: &[String], size: f64, available: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    for credit in credits {
        match lines.last_mut() {
            Some(line) if measure_text_width(&format!("{line}, {credit}"), size) <= available => {
                line.push_str(", ");
                line.push_str(credit);
            }
            _ => lines.push(credit.clone()),
        }
    }

    lines
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
    use std::{
        collections::{BTreeMap, HashMap},
        sync::Arc,
    };

    /// The burnt-in line and the web client's list are read side by side, so
    /// they have to agree: the caller's own credits, then OSM, then the datasets
    /// alphabetically, then the global fallback.
    #[test]
    fn credits_read_from_the_map_outwards() {
        let titles: BTreeMap<String, Vec<String>> = [
            ("osm", vec!["© OpenStreetMap contributors"]),
            ("shading:at", vec!["ALS DTM Austria"]),
            ("shading:sk", vec!["DMR 5.0: ÚGKK SR"]),
            // The same DEM contoured, which must not be named twice.
            ("contours:sk", vec!["DMR 5.0: ÚGKK SR"]),
            ("shading:_", vec!["GEDTM30"]),
        ]
        .into_iter()
        .map(|(code, titles)| {
            (
                code.to_owned(),
                titles.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect();

        let mut attribution = Attribution::default();
        attribution.add_shading("_");
        attribution.add_shading("sk");
        attribution.add_contours("sk");
        attribution.add_shading("at");
        attribution.add_osm();

        let decoration = AttributionDecoration {
            extra: vec!["OSRM / FOSSGIS e.\u{a0}V.".to_owned()],
            catalog: Arc::new(titles),
            // The app says OpenStreetMap's credit in the reader's language.
            overrides: [(
                "osm".to_owned(),
                "©\u{a0}prispievatelia OpenStreetMap".to_owned(),
            )]
            .into_iter()
            .collect(),
        };

        assert_eq!(
            compose_attribution(&decoration, &attribution),
            [
                FREEMAP,
                "OSRM / FOSSGIS e.\u{a0}V.",
                "©\u{a0}prispievatelia OpenStreetMap",
                "ALS DTM Austria",
                "DMR 5.0: ÚGKK SR",
                "GEDTM30",
            ]
        );
    }

    /// A code the catalog cannot name adds nothing to the drawn line: the export
    /// reports it separately, and a raw code means nothing burnt into a picture.
    #[test]
    fn an_unresolvable_code_adds_nothing() {
        let mut attribution = Attribution::default();
        attribution.add_shading("at");

        let decoration = AttributionDecoration {
            extra: Vec::new(),
            catalog: Arc::new(BTreeMap::new()),
            overrides: HashMap::new(),
        };

        assert_eq!(compose_attribution(&decoration, &attribution), [FREEMAP]);
    }

    #[test]
    #[allow(clippy::float_cmp)] // nice_distance returns exact 1/2/5 multiples
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
