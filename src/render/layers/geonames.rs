use crate::render::{
    Feature,
    collision::Collision,
    colors::{self},
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        text_on_line::{Distribution, TextOnLineOptions, place_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;
use cosmic_text::Style;
use geo::{Euclidean, Length, LineString};

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            name,
            geometry
        FROM
            geonames_smooth
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            minzoom <= $6 AND
            (maxzoom IS NULL OR maxzoom > $6)
        ORDER BY
            minzoom DESC,
            ST_Length(geometry) DESC,
            ogc_fid
    ";

    // Names avoid each other, so each tile must see every name that could block one drawn
    // in it, or neighbouring tiles would decide differently and cut names at their edges.
    client
        .query(
            sql,
            &ctx.bbox_query_params(Some(1024.0))
                .push(ctx.zoom as i32)
                .as_params(),
        )
        .await
}

/// Longest run of line per character the name is justified over, in ems.
const MAX_EM_PER_CHAR: f64 = 2.0;

/// How many name-lengths of line each repeat of the name covers.
const REPEAT_EVERY: f64 = 4.0;

/// Stretches of at most `len` to justify the name over: the whole line if short enough,
/// otherwise evenly spaced pieces, one per `REPEAT_EVERY` lengths of the name.
fn stretches(line: &LineString, len: f64) -> Vec<LineString> {
    let total = Euclidean.length(line);

    if total <= len {
        return vec![line.clone()];
    }

    let n = (total / (len * REPEAT_EVERY)).round().max(1.0);

    (0..n as usize)
        .map(|i| {
            let centre = (i as f64 + 0.5) * total / n;
            substring(line, centre - len / 2.0, centre + len / 2.0)
        })
        .collect()
}

fn substring(line: &LineString, from: f64, to: f64) -> LineString {
    let mut coords = Vec::new();
    let mut at = 0.0;

    for seg in line.lines() {
        let seg_len = Euclidean.length(&seg);

        if seg_len == 0.0 {
            continue;
        }

        let point = |d: f64| seg.start + (seg.end - seg.start) * ((d - at) / seg_len);

        if at + seg_len >= from && at <= to {
            coords.push(if at < from { point(from) } else { seg.start });

            if at + seg_len >= to {
                coords.push(point(to));
                break;
            }
        }

        at += seg_len;
    }

    LineString::new(coords)
}

/// Places the name on the stretch, or where another name blocks it on the middle, first or
/// last half of it, each at up to three sizes.
fn place(
    context: &Context,
    stretch: &LineString,
    name: &str,
    collision: &mut Collision,
    options: TextOnLineOptions,
) -> cairo::Result<()> {
    let total = Euclidean.length(stretch);

    let pieces = [
        stretch.clone(),
        substring(stretch, total / 4.0, total * 3.0 / 4.0),
        substring(stretch, 0.0, total / 2.0),
        substring(stretch, total / 2.0, total),
    ];

    for piece in &pieces {
        let mut options = options;

        for _ in 0..3 {
            if place_text_on_line(context, piece, name, Some(collision), &options)? {
                return Ok(());
            }

            options.flo.size *= 0.9;
            options.halo_width *= 0.9;
        }
    }

    Ok(())
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("geonames::render");

    let options = TextOnLineOptions {
        flo: FontAndLayoutOptions {
            // Each zoom shows its own level of units, so the size need not keep growing.
            size: (8.0 + 1.9f64.powf(ctx.zoom as f64 - 6.0)).clamp(13.0, 21.0),
            style: Style::Italic,
            ..Default::default()
        },
        distribution: Distribution::Justify { min_spacing: 0.0 },
        halo_opacity: 1.0,
        color: colors::TRAM,
        halo_width: 2.0,
        ..TextOnLineOptions::default()
    };

    context.push_group();

    // Names avoid only each other, not the labels beneath them. Rows come most specific
    // first, so a name carried over from lower zooms gives way to the units shown inside it.
    let mut collision = Collision::new(Some(context));

    for row in rows {
        let name = row.get_string("name")?;

        let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

        // A unit with no nested units keeps its label at every zoom, so its line can grow to
        // thousands of pixels; justified over all of it, the letters would drift apart.
        let len = name.chars().count() as f64 * options.flo.size * MAX_EM_PER_CHAR;

        for stretch in stretches(&geom, len) {
            place(context, &stretch, name, &mut collision, options)?;
        }
    }

    context.pop_group_to_source()?;

    context.paint_with_alpha(0.5)?;

    Ok(())
}
