use crate::render::{
    Feature, ctx::Ctx, layer_render_error::LayerRenderResult, projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use cairo::Context;
use geo::{Coord, LineString};

/// Bits of the `restriction` column.
const NO_BICYCLE: i32 = 1;
const NO_FOOT: i32 = 2;

/// How far from a junction node its cross is drawn, along the restricted way. Without it the
/// cross sits on the junction, where it reads as forbidding the joining way just as much.
const MARK_OFFSET: f64 = 10.0;

/// Crosses closer together than this collapse into one - which is what keeps a way shorter
/// than two offsets, or two junctions a metre apart, to a single cross.
const MERGE_GAP: f64 = 10.0;

/// Longest stretch left without a cross. Gaps longer than this are halved recursively, so the
/// crosses actually land between `REPEAT_SPACING / 2.0` and `REPEAT_SPACING` apart.
const REPEAT_SPACING: f64 = 300.0;

/// A stretch shorter than this carries one cross at its middle instead of one at each end. The
/// bar already says where the rule changes, so a single cross saying "this bit is restricted"
/// is enough wherever the whole stretch can be taken in at a glance. Half of
/// [`REPEAT_SPACING`] is the tightest the repeats themselves ever sit, so a stretch below it
/// has never earned two marks in the first place.
const SHORT_SEGMENT: f64 = REPEAT_SPACING / 2.0;

/// A stretch shorter than this gets no cross, which would cover the junctions at both its ends.
const MIN_STRETCH: f64 = 2.0 * MARK_OFFSET;

pub async fn query(
    ctx: &Ctx,
    client: &mut tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    // A cross goes at the start of every restricted stretch, which is either where the
    // restriction begins - the neighbouring way still allows what this way forbids, or there
    // is no neighbouring way at all - or where the way meets a Y or T junction, because a
    // traveller turning onto an arm there needs to be told what that arm forbids even when
    // the arm they came from forbade the same. `osm_roads` holds one row per OSM way, so the
    // topology is recoverable from the ways' shared vertices - `ST_Points` intersected with
    // `ST_Points` matches on exact coordinates, which two ways share only by referencing the
    // same OSM node. That finds junctions in the middle of a way as well as at its ends, and
    // ignores bridges crossing without a node. Railways are no route onto a way, so a level
    // crossing is neither a junction nor a place where a restriction starts.
    let sql = "
        WITH RECURSIVE r AS (
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
        -- The nodes of a restricted way that can end a stretch: the ones it shares with another
        -- way, plus its own two ends, which end a stretch even where nothing joins them.
        nodes AS (
            SELECT r.row_id, j.geom AS pt
            FROM
                restricted r
                JOIN osm_roads o ON
                    o.class <> 'railway' AND
                    o.osm_id <> r.osm_id AND
                    o.geometry && r.geometry
                CROSS JOIN LATERAL
                    ST_Dump(ST_Intersection(ST_Points(r.geometry), ST_Points(o.geometry))) j
            UNION
            SELECT r.row_id, p.pt
            FROM
                restricted r
                CROSS JOIN LATERAL (VALUES (ST_StartPoint(r.geometry)), (ST_EndPoint(r.geometry))) p(pt)
        ),
        -- The node's degree, split into the arms of the way itself and those of its neighbours,
        -- together with the bits the way forbids that some neighbour still allows.
        degrees AS (
            SELECT
                n.row_id,
                n.pt,
                r.restriction,
                road_arms(r.geometry, n.pt) + coalesce(other.arms, 0) AS degree,
                coalesce(other.starts, 0::smallint) AS starts,
                coalesce(other.stricter, 0::smallint) AS stricter,
                ST_LineLocatePoint(r.geometry, n.pt) AS frac,
                ST_IsClosed(r.geometry) AS closed,
                ST_Equals(n.pt, ST_StartPoint(r.geometry)) AS at_start,
                ST_Equals(n.pt, ST_EndPoint(r.geometry)) AS at_end,
                -- Where an end also lies on the way's own interior, the fraction of that inner pass.
                CASE
                    WHEN ST_IsClosed(r.geometry) OR road_arms(r.geometry, n.pt) < 3 THEN NULL
                    WHEN ST_Equals(n.pt, ST_EndPoint(r.geometry)) THEN ST_LineLocatePoint(r.geometry, n.pt)
                    ELSE 1 - ST_LineLocatePoint(ST_Reverse(r.geometry), n.pt)
                END AS inner_frac
            FROM
                nodes n
                JOIN restricted r ON r.row_id = n.row_id
                LEFT JOIN LATERAL (
                    SELECT
                        sum(road_arms(o.geometry, n.pt))::int AS arms,
                        bit_or(r.restriction & ~road_restriction(o.access, o.vehicle, o.bicycle, o.foot)) AS starts,
                        bit_or(road_restriction(o.access, o.vehicle, o.bicycle, o.foot) & ~r.restriction) AS stricter
                    FROM osm_roads o
                    WHERE
                        o.class <> 'railway' AND
                        o.osm_id <> r.osm_id AND
                        o.geometry && n.pt AND
                        ST_Intersects(ST_Points(o.geometry), n.pt)
                ) other ON true
        ),
        -- One cross per arm of the way at the node, so that a way passing through a junction is
        -- marked on both sides of it. `dir` says which way to step off the node to reach the
        -- arm; `bound` asks for the bar that says the rule changes at this very node.
        marks AS (
            SELECT
                d.row_id,
                m.frac,
                m.dir,
                -- A cross at a junction arm or a dead end needs no bar: the topology already
                -- says why it is there. Only a plain continuation of one way into the next
                -- hides its boundary, and only there does the restriction have to have
                -- actually changed - otherwise the stretch simply carries on.
                (d.degree = 2 AND d.starts <> 0) AS bound
            FROM
                degrees d
                CROSS JOIN LATERAL unnest(
                    CASE
                        -- The seam of a loop is both ends of the line at once, so the arm
                        -- running back into it is reached from the far end of the geometry.
                        WHEN d.closed AND d.at_start THEN ARRAY[0.0, 1.0]
                        WHEN d.at_start AND d.inner_frac IS NOT NULL THEN ARRAY[0.0, d.inner_frac, d.inner_frac]
                        WHEN d.at_start THEN ARRAY[0.0]
                        WHEN d.at_end AND d.inner_frac IS NOT NULL THEN ARRAY[d.inner_frac, d.inner_frac, 1.0]
                        WHEN d.at_end THEN ARRAY[1.0]
                        ELSE ARRAY[d.frac, d.frac]
                    END,
                    CASE
                        WHEN d.closed AND d.at_start THEN ARRAY[1, -1]
                        WHEN d.at_start AND d.inner_frac IS NOT NULL THEN ARRAY[1, -1, 1]
                        WHEN d.at_start THEN ARRAY[1]
                        WHEN d.at_end AND d.inner_frac IS NOT NULL THEN ARRAY[-1, 1, -1]
                        WHEN d.at_end THEN ARRAY[-1]
                        ELSE ARRAY[-1, 1]
                    END
                ) AS m(frac, dir)
            WHERE d.degree <> 2 OR d.starts <> 0
        ),
        spans AS (
            SELECT row_id, min(frac) AS first, max(frac) AS last
            FROM marks
            GROUP BY row_id
        ),
        -- How far a stretch runs on past an unmarked end of its way before something bounds it on
        -- the ways it continues into. Walked on `osm_roads` so every tile gets the same answer;
        -- only ends already within $6 of their way's last mark can lead to a short stretch.
        walk (row_id, at_start, from_id, pt, acc, depth, ext) AS (
            SELECT
                d.row_id,
                d.at_start,
                r.osm_id,
                d.pt,
                0.0::float8,
                0,
                -- A way forbidding more marks this node itself, so the stretch ends right here.
                CASE WHEN d.stricter <> 0 THEN 0.0::float8 END
            FROM
                degrees d
                JOIN restricted r ON r.row_id = d.row_id
                LEFT JOIN spans sp ON sp.row_id = d.row_id
            WHERE
                (d.at_start OR d.at_end) AND
                NOT d.closed AND
                d.degree = 2 AND
                d.starts = 0 AND
                ST_Length(r.geometry) * CASE
                    WHEN d.at_start THEN coalesce(sp.first, 1)
                    ELSE 1 - coalesce(sp.last, 0)
                END < $6
            UNION ALL
            SELECT
                w.row_id,
                w.at_start,
                s.osm_id,
                s.far,
                w.acc + s.len,
                w.depth + 1,
                w.acc + coalesce(s.bound_at, CASE WHEN s.continues THEN NULL ELSE s.len END)
            FROM
                walk w
                CROSS JOIN LATERAL (
                    SELECT
                        o.osm_id,
                        ST_Length(o.geometry) AS len,
                        e.far,
                        -- The nearest junction inside the way ends the stretch before its far end.
                        (
                            SELECT
                                min(abs(
                                    CASE
                                        WHEN ST_Equals(v.geom, ST_StartPoint(o.geometry))
                                            THEN 1 - ST_LineLocatePoint(ST_Reverse(o.geometry), v.geom)
                                        ELSE ST_LineLocatePoint(o.geometry, v.geom)
                                    END - e.fp
                                )) * ST_Length(o.geometry)
                            FROM ST_DumpPoints(o.geometry) v
                            WHERE
                                v.path[1] > 1 AND
                                v.path[1] < ST_NPoints(o.geometry) AND
                                ST_DWithin(v.geom, w.pt, $6 - w.acc) AND
                                (
                                    -- The way's own end resting on this vertex makes it a junction too.
                                    ST_Equals(v.geom, ST_StartPoint(o.geometry)) OR
                                    ST_Equals(v.geom, ST_EndPoint(o.geometry)) OR
                                    EXISTS (
                                    SELECT 1
                                    FROM osm_roads x
                                    WHERE
                                        x.class <> 'railway' AND
                                        x.osm_id <> o.osm_id AND
                                        x.geometry && v.geom AND
                                        ST_Intersects(ST_Points(x.geometry), v.geom)
                                    )
                                )
                        ) AS bound_at,
                        (
                            SELECT
                                count(*) = 1 AND
                                bool_and(
                                    road_arms(q.geometry, e.far) = 1 AND
                                    road_restriction(o.access, o.vehicle, o.bicycle, o.foot)
                                        = road_restriction(q.access, q.vehicle, q.bicycle, q.foot)
                                )
                            FROM osm_roads q
                            WHERE
                                q.class <> 'railway' AND
                                q.osm_id <> o.osm_id AND
                                q.geometry && e.far AND
                                ST_Intersects(ST_Points(q.geometry), e.far)
                        ) AS continues
                    FROM
                        osm_roads o
                        CROSS JOIN LATERAL (
                            SELECT
                                ST_LineLocatePoint(o.geometry, w.pt) AS fp,
                                CASE
                                    WHEN ST_Equals(w.pt, ST_StartPoint(o.geometry)) THEN ST_EndPoint(o.geometry)
                                    ELSE ST_StartPoint(o.geometry)
                                END AS far
                        ) e
                    WHERE
                        o.class <> 'railway' AND
                        -- Not drawn by this layer, so it could never take the stretch's cross.
                        o.type NOT IN ('trunk', 'motorway', 'trunk_link', 'motorway_link') AND
                        o.osm_id <> w.from_id AND
                        NOT ST_IsClosed(o.geometry) AND
                        o.geometry && w.pt AND
                        ST_Intersects(ST_Points(o.geometry), w.pt)
                    -- Keeps the planner from inlining the columns and computing each twice.
                    OFFSET 0
                ) s
            -- The length cap ends every walk alike; a low depth limit would stop walks from a
            -- chain's ends sooner than from its middle, so it only guards zero-length loops.
            WHERE w.ext IS NULL AND w.depth < 1000 AND w.acc < $6
        ),
        exts AS (
            SELECT
                row_id,
                max(ext) FILTER (WHERE at_start) AS ext_start,
                max(ext) FILTER (WHERE NOT at_start) AS ext_end
            FROM walk
            GROUP BY row_id
        )
        SELECT
            r.geometry,
            r.restriction::int AS restriction,
            coalesce(array_agg(m.frac ORDER BY m.frac, m.dir) FILTER (WHERE m.row_id IS NOT NULL), '{}') AS mark_fracs,
            coalesce(array_agg(m.dir ORDER BY m.frac, m.dir) FILTER (WHERE m.row_id IS NOT NULL), '{}') AS mark_dirs,
            coalesce(array_agg(m.bound::int ORDER BY m.frac, m.dir) FILTER (WHERE m.row_id IS NOT NULL), '{}') AS mark_bounds,
            coalesce(max(x.ext_start), 'Infinity') AS ext_start,
            coalesce(max(x.ext_end), 'Infinity') AS ext_end
        FROM
            restricted r
            LEFT JOIN marks m ON m.row_id = r.row_id
            LEFT JOIN exts x ON x.row_id = r.row_id
        GROUP BY r.row_id, r.geometry, r.restriction
    ";

    let short = ctx.meters_per_pixel() * SHORT_SEGMENT;
    let bbox = ctx.bbox_query_params(Some(32.0));
    let mut params = bbox.as_params();

    params.push(&short);

    let tx = client.transaction().await?;

    // The planner costs this query in the billions whatever the tile holds, so JIT would compile
    // it on every tile, adding ~1.7 s that the query itself never needs.
    tx.batch_execute("SET LOCAL jit = off").await?;

    let rows = tx.query(sql, &params).await?;

    tx.commit().await?;

    Ok(rows)
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
fn fill_gap(from: f64, to: f64, marks: &mut Vec<f64>) {
    if to - from <= REPEAT_SPACING {
        return;
    }

    let mid = f64::midpoint(from, to);

    marks.push(mid);

    fill_gap(from, mid, marks);
    fill_gap(mid, to, marks);
}

/// Turns the marks the query found on a way into pixel positions along its projected geometry,
/// merges the crosses that would overlap, then fills the gaps left between them.
///
/// Returns the crosses and, separately, the bars - a bar stays on its node, where the rule
/// actually changes, while the cross it belongs to is pushed [`MARK_OFFSET`] into the arm it
/// speaks for, or replaced by a single one in the middle when the whole stretch is short, or
/// dropped with its bars when the stretch is too short to mark at all.
///
/// `ext` is how far, in map units, the stretch runs on past an unmarked start and end of the way
/// (infinite when unknown or long).
fn place_marks(
    fracs: &[f64],
    dirs: &[i32],
    bounds: &[i32],
    ext: (f64, f64),
    lengths: &[f64],
    map_lengths: &[f64],
) -> (Vec<f64>, Vec<f64>) {
    let total = *lengths.last().expect("non-empty line");
    let map_total = *map_lengths.last().expect("non-empty line");

    let mut nodes = Vec::<(f64, i32, bool, bool)>::with_capacity(fracs.len() + 2);

    for ((frac, dir), bound) in fracs.iter().zip(dirs).zip(bounds) {
        // `frac` is a fraction of the length in map units; the tile projection scales the axes
        // separately, so it is converted through the segment it falls in rather than scaled.
        let i = segment_at(map_lengths, frac * map_total);
        let map_segment = map_lengths[i + 1] - map_lengths[i];

        let t = if map_segment > 0.0 {
            (frac * map_total - map_lengths[i]) / map_segment
        } else {
            0.0
        };

        let node = t.mul_add(lengths[i + 1] - lengths[i], lengths[i]);

        nodes.push((node, *dir, *bound != 0, true));
    }

    // The mark bounding the stretch on a neighbouring way stands in as a partner that draws nothing.
    let px_per_unit = total / map_total;

    if ext.0.is_finite() {
        nodes.push((-ext.0 * px_per_unit, 1, false, false));
    }

    if ext.1.is_finite() {
        nodes.push((ext.1.mul_add(px_per_unit, total), -1, false, false));
    }

    // A stretch of the way runs from a mark that opens it to the next one, which closes it, so
    // the two are a `1` followed by a `-1`. Walking the marks in that order is what keeps the
    // collapse below from chaining: each mark is spent on at most one stretch, whereas simply
    // widening `MERGE_GAP` would fold a run of close junctions into one cross in the middle of
    // a long way, far from any arm it speaks for.
    nodes.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut crosses = Vec::<f64>::with_capacity(nodes.len());
    let mut bars = Vec::<f64>::new();
    let mut i = 0;

    while i < nodes.len() {
        let (node, dir, bound, real) = nodes[i];

        // A stretch that leaves this way has no mark closing it here, so it never looks short.
        if let Some(&(next, -1, next_bound, _)) = nodes.get(i + 1)
            && dir == 1
            && next - node < SHORT_SEGMENT
        {
            // A bar without its cross would not say which side it governs, so both go together.
            if next - node >= MIN_STRETCH {
                let mid = f64::midpoint(node, next);

                // A stretch spanning ways is marked by the one its middle falls on.
                if (0.0..=total).contains(&mid) {
                    crosses.push(mid);
                }

                if bound {
                    bars.push(node);
                }

                if next_bound {
                    bars.push(next);
                }
            }

            i += 2;

            continue;
        }

        if real {
            if bound {
                bars.push(node);
            }

            crosses.push(f64::from(dir).mul_add(MARK_OFFSET, node).clamp(0.0, total));
        }

        i += 1;
    }

    crosses.sort_unstable_by(f64::total_cmp);

    let mut merged = Vec::<f64>::with_capacity(crosses.len());

    for pos in crosses {
        match merged.last_mut() {
            Some(last) if pos - *last < MERGE_GAP => *last = f64::midpoint(*last, pos),
            _ => merged.push(pos),
        }
    }

    // Snapshotted, because the gaps are filled into the very vector being walked.
    let boundaries = merged.clone();

    let mut prev = 0.0;

    for pos in boundaries {
        fill_gap(prev, pos, &mut merged);

        prev = pos;
    }

    fill_gap(prev, total, &mut merged);

    // Two bars on one spot would darken each other. `bound` cannot produce that today - it
    // needs a node of degree two, which on this way has a single arm and so a single mark -
    // but the guard is a sort of a handful of floats and it survives the condition widening.
    bars.sort_unstable_by(f64::total_cmp);
    bars.dedup_by(|a, b| (*a - *b).abs() < 0.5);

    (merged, bars)
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

    let boundary_icon = &svg_repo.get("access_boundary")?.clone();

    let boundary_rect = boundary_icon.extents().expect("surface extents");

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

        // Every cross shows the way's whole restriction, so its colour always answers the one
        // question "what is forbidden on the line under me"; the bar alone says "and the rule
        // changes right here".
        let (icon, rect) = match restriction & (NO_BICYCLE | NO_FOOT) {
            0 => continue,
            NO_BICYCLE => (no_bicycle_icon, no_bicycle_rect),
            NO_FOOT => (no_foot_icon, no_foot_rect),
            _ => (no_foot_bicycle_icon, no_foot_bicycle_rect),
        };

        let (crosses, bars) = place_marks(
            &row.get_f64_array("mark_fracs")?,
            &row.get_i32_array("mark_dirs")?,
            &row.get_i32_array("mark_bounds")?,
            (row.get_f64("ext_start")?, row.get_f64("ext_end")?),
            &lengths,
            &map_lengths,
        );

        for (pos, (icon, rect)) in crosses.into_iter().map(|pos| (pos, (icon, rect))).chain(
            bars.into_iter()
                .map(|pos| (pos, (boundary_icon, boundary_rect))),
        ) {
            let (x, y, angle) = sample(&geom, &lengths, pos);

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

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: (f64, f64) = (f64::INFINITY, f64::INFINITY);

    /// Places the crosses on a straight way, where a fraction along it is that fraction of
    /// `len` pixels. Every `len` here stays under [`REPEAT_SPACING`] so that `fill_gap` adds
    /// nothing and the assertions see only what `place_marks` itself decided.
    fn crosses(fracs: &[f64], dirs: &[i32], len: f64) -> Vec<f64> {
        assert!(
            len <= REPEAT_SPACING,
            "a longer way would gain repeat marks"
        );

        let lengths = vec![0.0, len];
        let bounds = vec![0; fracs.len()];

        place_marks(fracs, dirs, &bounds, OPEN, &lengths, &lengths).0
    }

    #[track_caller]
    fn assert_marks(got: &[f64], want: &[f64]) {
        assert_eq!(got.len(), want.len(), "got {got:?}, want {want:?}");

        for (got, want) in got.iter().zip(want) {
            assert!((got - want).abs() < 1e-9, "got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn short_stretch_collapses_to_one_cross_in_the_middle() {
        // Marks 45 px apart: the stretch between them is short, so instead of a cross at each
        // end it gets a single one halfway.
        assert_marks(&crosses(&[0.1, 0.25], &[1, -1], 300.0), &[52.5]);
    }

    #[test]
    fn long_stretch_keeps_a_cross_at_each_end() {
        // The same pair 195 px apart stays two crosses, each `MARK_OFFSET` into the stretch.
        assert_marks(&crosses(&[0.1, 0.75], &[1, -1], 300.0), &[40.0, 215.0]);
    }

    #[test]
    fn collapsing_does_not_chain_across_a_run_of_close_junctions() {
        // Four junctions 30 px apart, each passed straight through, so every one contributes a
        // `-1` closing the stretch behind it and a `1` opening the one ahead. Widening
        // `MERGE_GAP` would fold the lot into a single cross; pairing collapses each stretch on
        // its own, leaving a mark on every one of them.
        let fracs = [0.1, 0.1, 0.2, 0.2, 0.3, 0.3, 0.4, 0.4];
        let dirs = [-1, 1, -1, 1, -1, 1, -1, 1];

        assert_marks(
            &crosses(&fracs, &dirs, 300.0),
            &[20.0, 45.0, 75.0, 105.0, 130.0],
        );
    }

    #[test]
    fn a_stretch_too_short_to_mark_gets_neither_cross_nor_bar() {
        let lengths = vec![0.0, 300.0];
        let mark = |len: f64| {
            place_marks(
                &[0.1, 0.1 + len / 300.0],
                &[1, -1],
                &[1, 0],
                OPEN,
                &lengths,
                &lengths,
            )
        };

        let (crosses, bars) = mark(MIN_STRETCH - 1.0);

        assert!(crosses.is_empty(), "{crosses:?}");
        assert!(bars.is_empty(), "{bars:?}");

        let (crosses, bars) = mark(MIN_STRETCH + 1.0);

        assert_eq!((crosses.len(), bars.len()), (1, 1));
    }

    #[test]
    fn a_stretch_just_over_the_threshold_still_gets_both_ends() {
        // Guards the cut itself: one pixel either side of `SHORT_SEGMENT` changes the answer.
        let len = 300.0;
        let over = (SHORT_SEGMENT + 1.0) / len;
        let under = (SHORT_SEGMENT - 1.0) / len;

        assert_eq!(crosses(&[0.0, over], &[1, -1], len).len(), 2);
        assert_eq!(crosses(&[0.0, under], &[1, -1], len).len(), 1);
    }

    #[test]
    fn a_stretch_leaving_the_way_is_never_short() {
        // A lone `-1`: the stretch it closes started on some earlier way, so there is nothing
        // to pair it with and its cross stays `MARK_OFFSET` back from the node.
        assert_marks(&crosses(&[0.5], &[-1], 200.0), &[90.0]);
    }

    #[test]
    fn a_bar_stays_on_its_node_when_the_stretch_collapses() {
        let lengths = vec![0.0, 300.0];

        let (crosses, bars) =
            place_marks(&[0.1, 0.25], &[1, -1], &[1, 0], OPEN, &lengths, &lengths);

        assert_marks(&crosses, &[52.5]);
        assert_marks(&bars, &[30.0]);
    }

    #[test]
    fn a_stretch_spanning_two_ways_is_marked_once_by_the_way_holding_its_middle() {
        // A junction 10 px before the end of `a`, which runs on into `b`, a 30 px dead end.
        let a = vec![0.0, 100.0];
        let b = vec![0.0, 30.0];

        let on_a = place_marks(&[0.9], &[1], &[0], (f64::INFINITY, 30.0), &a, &a).0;
        let on_b = place_marks(&[1.0], &[-1], &[0], (10.0, f64::INFINITY), &b, &b).0;

        assert_marks(&on_a, &[]);
        assert_marks(&on_b, &[10.0]);
    }

    #[test]
    fn a_stretch_spanning_two_ways_too_short_to_mark_gets_nothing() {
        let a = vec![0.0, 100.0];
        let b = vec![0.0, 5.0];

        let on_a = place_marks(&[0.9], &[1], &[0], (f64::INFINITY, 5.0), &a, &a).0;
        let on_b = place_marks(&[1.0], &[-1], &[0], (10.0, f64::INFINITY), &b, &b).0;

        assert_marks(&on_a, &[]);
        assert_marks(&on_b, &[]);
    }

    #[test]
    fn a_long_stretch_spanning_ways_keeps_its_offset_cross() {
        let a = vec![0.0, 100.0];

        assert_marks(
            &place_marks(&[0.5], &[1], &[0], (f64::INFINITY, 200.0), &a, &a).0,
            &[60.0],
        );
    }
}
