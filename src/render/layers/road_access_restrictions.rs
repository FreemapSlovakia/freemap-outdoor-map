use crate::render::{
    Feature, ctx::Ctx, layer_render_error::LayerRenderResult, projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use cairo::Context;
use geo::{Coord, LineString};

/// Bits of the `restriction` column and of every entry of `mark_bits`.
const NO_BICYCLE: i32 = 1;
const NO_FOOT: i32 = 2;

/// How far from a junction node its cross is drawn, along the restricted way. Without it the
/// cross sits on the junction, where it reads as forbidding the joining way just as much.
const MARK_OFFSET: f64 = 10.0;

/// Crosses closer together than this collapse into one carrying both their modes - which is
/// what keeps a way shorter than two offsets, or two junctions a metre apart, to one cross.
const MERGE_GAP: f64 = 10.0;

/// Longest stretch left without a cross. Gaps longer than this are halved recursively, so the
/// crosses actually land between `REPEAT_SPACING / 2.0` and `REPEAT_SPACING` apart.
const REPEAT_SPACING: f64 = 300.0;

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    // Crosses go where a restriction *starts*: at a shared node whose other ways still allow
    // what this way forbids, and at dead ends. `osm_roads` holds one row per OSM way, so the
    // topology is recoverable from the ways' shared vertices - `ST_Points` intersected with
    // `ST_Points` matches on exact coordinates, which two ways share only by referencing the
    // same OSM node. That finds junctions in the middle of a way as well as at its ends, and
    // ignores bridges crossing without a node. Railways are no route onto a way, so a level
    // crossing is not a place where a restriction starts.
    let sql = "
        WITH r AS (
            SELECT
                -- Row identity: with `-limitto`, one OSM way can be clipped into several rows.
                ctid AS row_id,
                osm_id,
                geometry,
                road_restriction(access, vehicle, bicycle, foot) AS restriction
            FROM
                osm_roads
            WHERE
                type NOT IN ('trunk', 'motorway', 'trunk_link', 'motorway_link') AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ),
        restricted AS (
            SELECT * FROM r WHERE restriction <> 0
        ),
        marks AS (
            SELECT row_id, osm_id, pt, bit_or(starts) AS starts
            FROM (
                SELECT
                    r.row_id,
                    r.osm_id,
                    j.geom AS pt,
                    r.restriction & ~road_restriction(o.access, o.vehicle, o.bicycle, o.foot) AS starts
                FROM
                    restricted r
                    JOIN osm_roads o ON
                        o.class <> 'railway' AND
                        o.osm_id <> r.osm_id AND
                        o.geometry && r.geometry
                    CROSS JOIN LATERAL
                        ST_Dump(ST_Intersection(ST_Points(r.geometry), ST_Points(o.geometry))) j
                UNION ALL
                SELECT r.row_id, r.osm_id, p.pt, r.restriction
                FROM
                    restricted r
                    CROSS JOIN LATERAL (VALUES (ST_StartPoint(r.geometry)), (ST_EndPoint(r.geometry))) p(pt)
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM osm_roads o
                    WHERE
                        o.class <> 'railway' AND
                        o.osm_id <> r.osm_id AND
                        o.geometry && p.pt AND
                        ST_Intersects(o.geometry, p.pt)
                )
            ) m
            GROUP BY row_id, osm_id, pt
            HAVING bit_or(starts) <> 0
        ),
        -- Where two restricted ways meet an unrestricted one, both would mark the same node.
        deduped AS (
            SELECT
                (array_agg(row_id ORDER BY osm_id))[1] AS row_id,
                pt,
                bit_or(starts) AS starts
            FROM marks
            GROUP BY pt
        )
        SELECT
            r.geometry,
            r.restriction::int AS restriction,
            coalesce(array_agg(f.frac ORDER BY f.frac) FILTER (WHERE d.pt IS NOT NULL), '{}') AS mark_fracs,
            coalesce(array_agg(d.starts::int ORDER BY f.frac) FILTER (WHERE d.pt IS NOT NULL), '{}') AS mark_bits
        FROM
            restricted r
            LEFT JOIN deduped d ON d.row_id = r.row_id
            LEFT JOIN LATERAL (SELECT ST_LineLocatePoint(r.geometry, d.pt) AS frac) f ON true
        GROUP BY r.row_id, r.geometry, r.restriction
    ";

    client
        .query(sql, &ctx.bbox_query_params(Some(32.0)).as_params())
        .await
}

/// A cross to draw, at `pos` pixels along the way, for the modes in `bits`.
struct Mark {
    pos: f64,
    bits: i32,
}

/// Distance of every vertex from the start of the line.
fn cumulative_lengths(line_string: &LineString) -> Vec<f64> {
    let mut lengths = Vec::with_capacity(line_string.0.len());
    let mut total = 0.0;
    let mut prev: Option<Coord> = None;

    for coord in &line_string.0 {
        if let Some(prev) = prev {
            total += (coord.x - prev.x).hypot(coord.y - prev.y);
        }

        lengths.push(total);
        prev = Some(*coord);
    }

    lengths
}

/// Index of the segment containing `distance`, as an index into the coordinates.
fn segment_at(lengths: &[f64], distance: f64) -> usize {
    lengths
        .partition_point(|length| *length <= distance)
        .clamp(1, lengths.len() - 1)
        - 1
}

/// Position and bearing at `distance` pixels along the line.
fn sample(line_string: &LineString, lengths: &[f64], distance: f64) -> (f64, f64, f64) {
    let i = segment_at(lengths, distance);
    let (from, to) = (line_string.0[i], line_string.0[i + 1]);
    let segment = lengths[i + 1] - lengths[i];

    let t = if segment > 0.0 {
        (distance - lengths[i]) / segment
    } else {
        0.0
    };

    (
        t.mul_add(to.x - from.x, from.x),
        t.mul_add(to.y - from.y, from.y),
        (to.y - from.y).atan2(to.x - from.x),
    )
}

/// Fills a gap between two crosses by halving it until the halves fit within
/// [`REPEAT_SPACING`], so that a long restricted way still says so where it is looked at.
fn fill_gap(from: f64, to: f64, bits: i32, marks: &mut Vec<Mark>) {
    if to - from <= REPEAT_SPACING {
        return;
    }

    let mid = f64::midpoint(from, to);

    marks.push(Mark { pos: mid, bits });

    fill_gap(from, mid, bits, marks);
    fill_gap(mid, to, bits, marks);
}

/// Turns the marks the query found on a way into pixel positions along its projected geometry,
/// merges the ones that would overlap, then fills the gaps left between them.
fn place_marks(
    fracs: &[f64],
    bits: &[i32],
    restriction: i32,
    lengths: &[f64],
    map_lengths: &[f64],
) -> Vec<Mark> {
    let total = *lengths.last().expect("non-empty line");
    let map_total = *map_lengths.last().expect("non-empty line");

    let mut marks = Vec::<Mark>::with_capacity(fracs.len());

    for (frac, bits) in fracs.iter().zip(bits) {
        // `frac` is a fraction of the length in map units; the tile projection scales the axes
        // separately, so it is converted through the segment it falls in rather than scaled.
        let i = segment_at(map_lengths, frac * map_total);
        let map_segment = map_lengths[i + 1] - map_lengths[i];

        let t = if map_segment > 0.0 {
            (frac * map_total - map_lengths[i]) / map_segment
        } else {
            0.0
        };

        let pos = t.mul_add(lengths[i + 1] - lengths[i], lengths[i]);

        // Away from the node, into the restricted way - which for its far end means backwards.
        let pos = if *frac > 0.5 {
            pos - MARK_OFFSET
        } else {
            pos + MARK_OFFSET
        };

        marks.push(Mark {
            pos: pos.clamp(0.0, total),
            bits: *bits,
        });
    }

    marks.sort_unstable_by(|a, b| a.pos.total_cmp(&b.pos));

    let mut merged = Vec::<Mark>::with_capacity(marks.len());

    for mark in marks {
        match merged.last_mut() {
            Some(last) if mark.pos - last.pos < MERGE_GAP => {
                last.pos = f64::midpoint(last.pos, mark.pos);
                last.bits |= mark.bits;
            }
            _ => merged.push(mark),
        }
    }

    // Snapshotted, because the gaps are filled into the very vector being walked.
    let boundaries = merged.iter().map(|mark| mark.pos).collect::<Vec<_>>();

    let mut prev = 0.0;

    for pos in boundaries {
        fill_gap(prev, pos, restriction, &mut merged);

        prev = pos;
    }

    fill_gap(prev, total, restriction, &mut merged);

    merged
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("road_access_restrictions::render");

    // TODO lazy

    let no_bicycle_icon = &svg_repo.get("no_bicycle")?.clone();

    let no_foot_icon = &svg_repo.get("no_foot")?.clone();

    let no_foot_bicycle_icon = &svg_repo.get("no_foot_bicycle")?.clone();

    let no_bicycle_rect = no_bicycle_icon.extents().expect("surface extents");

    let no_foot_rect = no_foot_icon.extents().expect("surface extents");

    let no_foot_bicycle_rect = no_foot_bicycle_icon.extents().expect("surface extents");

    for row in rows {
        let restriction = row.get_i32("restriction")?;

        if restriction == 0 {
            continue;
        }

        let map_geom = row.get_line_string()?;

        if map_geom.0.len() < 2 {
            continue;
        }

        let geom = map_geom.project_to_tile(&ctx.tile_projector);

        let lengths = cumulative_lengths(&geom);
        let map_lengths = cumulative_lengths(&map_geom);

        if *lengths.last().expect("non-empty line") <= 0.0
            || *map_lengths.last().expect("non-empty line") <= 0.0
        {
            continue;
        }

        let marks = place_marks(
            &row.get_f64_array("mark_fracs")?,
            &row.get_i32_array("mark_bits")?,
            restriction,
            &lengths,
            &map_lengths,
        );

        for mark in marks {
            let (icon, rect) = match mark.bits & (NO_BICYCLE | NO_FOOT) {
                0 => continue,
                NO_BICYCLE => (no_bicycle_icon, no_bicycle_rect),
                NO_FOOT => (no_foot_icon, no_foot_rect),
                _ => (no_foot_bicycle_icon, no_foot_bicycle_rect),
            };

            let (x, y, angle) = sample(&geom, &lengths, mark.pos);

            context.save()?;
            context.translate(x, y);
            context.rotate(angle);
            context.set_source_surface(icon, -rect.width() / 2.0, -rect.height() / 2.0)?;
            context.paint_with_alpha(0.75)?;
            context.restore()?;
        }
    }

    Ok(())
}
