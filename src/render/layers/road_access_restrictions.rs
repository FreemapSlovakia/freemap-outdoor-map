use crate::render::{
    Feature,
    ctx::Ctx,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::{SvgRepo, SvgRepoError},
};
use cairo::Context;
use geo::{BoundingRect, Coord, LineString};
use std::collections::HashMap;

const NO_BICYCLE: u8 = 1;
const NO_FOOT: u8 = 2;

/// Keeps a cross off its junction, where it would read as forbidding the joining way too.
const MARK_OFFSET: f64 = 10.0;

/// Crosses closer together than this collapse into one.
const MERGE_GAP: f64 = 10.0;

/// Spacing of the crosses repeated along a restricted way.
const REPEAT_SPACING: f64 = 300.0;

/// A stretch shorter than this gets one cross in its middle; a repeat gives way to marks this close.
const SHORT_SEGMENT: f64 = REPEAT_SPACING / 2.0;

/// A stretch shorter than this gets no cross, which would cover the junctions at both its ends.
const MIN_STRETCH: f64 = 2.0 * MARK_OFFSET;

/// How far outside the tile a mark can still show.
const DRAW_BUFFER: f64 = 32.0;

/// Covers every node a visible mark can depend on, so each gets its complete degree.
const TOPOLOGY_BUFFER: f64 = DRAW_BUFFER + MERGE_GAP + MARK_OFFSET + SHORT_SEGMENT;

const UNDRAWN_TYPES: [&str; 4] = ["trunk", "motorway", "trunk_link", "motorway_link"];

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT osm_id, geometry, type, class, access, vehicle, bicycle, foot
        FROM osm_roads
        WHERE geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
    ";

    client
        .query(
            sql,
            &ctx.bbox_query_params(Some(TOPOLOGY_BUFFER)).as_params(),
        )
        .await
}

fn access_denied(value: &str) -> bool {
    !matches!(value, "" | "yes" | "designated" | "official" | "permissive")
}

/// Restriction bits of a road; an unknown access value counts as forbidden.
fn restriction(access: &str, vehicle: &str, bicycle: &str, foot: &str) -> u8 {
    let mut bits = 0;

    if access_denied(bicycle)
        || (bicycle.is_empty()
            && (access_denied(vehicle) || (vehicle.is_empty() && access_denied(access))))
    {
        bits |= NO_BICYCLE;
    }

    if access_denied(foot) || (foot.is_empty() && access_denied(access)) {
        bits |= NO_FOOT;
    }

    bits
}

struct Road {
    /// Vertices by coordinate bits: ways share a node only by referencing the same OSM node.
    keys: Vec<(u64, u64)>,
    geom: LineString,
    lengths: Vec<f64>,
    bits: u8,
    /// Railways never count as a way at a node.
    joins: bool,
    draws: bool,
}

impl Road {
    fn new(map: &LineString, geom: LineString, typ: &str, class: &str, bits: u8) -> Option<Self> {
        let lengths = cumulative_lengths(&geom);

        if *lengths.last()? <= 0.0 {
            return None;
        }

        Some(Self {
            keys: map
                .0
                .iter()
                .map(|c| (c.x.to_bits(), c.y.to_bits()))
                .collect(),
            geom,
            lengths,
            bits,
            joins: class != "railway",
            draws: bits != 0 && !UNDRAWN_TYPES.contains(&typ),
        })
    }

    fn total(&self) -> f64 {
        *self.lengths.last().expect("non-empty line")
    }

    const fn last(&self) -> usize {
        self.keys.len() - 1
    }

    /// Ways lead away from a vertex once where this way ends there, twice where it passes through.
    const fn arms(&self, vertex: usize) -> u32 {
        if vertex == 0 || vertex == self.last() {
            1
        } else {
            2
        }
    }
}

/// Orders the rows by OSM way, so that overlapping marks stack alike on every tile, and merges rows
/// of one way with identical vertices (a street that is also a tram line) into one road.
fn merge_rows(mut rows: Vec<(Option<i64>, Road)>) -> Vec<Road> {
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.keys.cmp(&b.1.keys)));

    let mut roads = Vec::<Road>::with_capacity(rows.len());
    let mut last_id = None;

    for (id, road) in rows {
        if id.is_some()
            && id == last_id
            && let Some(prev) = roads.last_mut()
            && prev.keys == road.keys
        {
            prev.joins |= road.joins;
            prev.draws |= road.draws;

            continue;
        }

        last_id = id;
        roads.push(road);
    }

    roads
}

type NodeIndex = HashMap<(u64, u64), Vec<(usize, usize)>>;

/// A mark at `pos` pixels along a way, opening the stretch ahead (`1`) or closing the one behind.
#[derive(Clone, Copy, Debug)]
struct Mark {
    pos: f64,
    dir: i8,
    bound: bool,
}

#[derive(Clone, Copy, Debug)]
enum End {
    /// The end carries a mark of its own.
    Marked,
    /// A way forbidding more meets it here and marks the node itself.
    Stricter,
    /// The stretch runs on into this vertex, an end of a way with the same restriction.
    Continues(usize, usize),
}

struct Topology {
    /// Ascending by position.
    marks: Vec<Mark>,
    ends: [End; 2],
}

fn topology(roads: &[Road], index: &NodeIndex, r: usize) -> Topology {
    let road = &roads[r];
    let mut marks = Vec::new();
    let mut ends = [End::Marked; 2];

    for (v, key) in road.keys.iter().enumerate() {
        let mut degree = 0;
        let mut starts = 0;
        let mut stricter = 0;
        let mut other = None;

        for &(o, ov) in &index[key] {
            if o != r && !roads[o].joins {
                continue;
            }

            degree += roads[o].arms(ov);

            if o != r {
                starts |= road.bits & !roads[o].bits;
                stricter |= roads[o].bits & !road.bits;
                other = Some((o, ov));
            }
        }

        // A continuation is a stretch boundary only where the restriction changes.
        let is_mark = degree != 2 || starts != 0;
        let pos = road.lengths[v];

        if is_mark {
            // Only a continuation hides its boundary, so only there does a bar show it.
            let bound = degree == 2;

            if v != 0 {
                marks.push(Mark {
                    pos,
                    dir: -1,
                    bound,
                });
            }

            if v != road.last() {
                marks.push(Mark { pos, dir: 1, bound });
            }
        }

        if v == 0 || v == road.last() {
            ends[usize::from(v != 0)] = if is_mark {
                End::Marked
            } else if stricter != 0 {
                End::Stricter
            } else {
                // Nothing else here means the seam of a loop, which runs on into its other end.
                let (o, ov) = other.unwrap_or_else(|| (r, road.last() - v));

                End::Continues(o, ov)
            };
        }
    }

    Topology { marks, ends }
}

/// How far a stretch runs on past a way's end before a mark bounds it; infinite once that reaches a
/// short stretch, or when it runs onto a way this layer never draws.
fn reach(roads: &[Road], topos: &[Option<Topology>], mut end: End) -> f64 {
    let mut acc = 0.0;

    // Bounds a loop of mark-less ways.
    for _ in 0..=roads.len() {
        let (road, vertex) = match end {
            End::Marked => return f64::INFINITY,
            End::Stricter => return acc,
            End::Continues(road, vertex) => (road, vertex),
        };

        let Some(topo) = &topos[road] else {
            return f64::INFINITY;
        };

        let total = roads[road].total();
        let entered_at_start = vertex == 0;

        let nearest = if entered_at_start {
            topo.marks.first().map(|mark| mark.pos)
        } else {
            topo.marks.last().map(|mark| total - mark.pos)
        };

        if let Some(distance) = nearest {
            return acc + distance;
        }

        acc += total;

        if acc >= SHORT_SEGMENT {
            return f64::INFINITY;
        }

        end = topo.ends[usize::from(entered_at_start)];
    }

    f64::INFINITY
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

/// Crosses and bars for a way's marks, given how far its stretches run on past its start and end
/// (`ext`): a short stretch gets one cross in its middle, and one too short for any gets nothing.
fn place_marks(marks: &[Mark], ext: (f64, f64), total: f64) -> (Vec<f64>, Vec<f64>) {
    // Whether each mark is real; a stand-in for the mark ending the stretch on another way draws nothing.
    let mut nodes = marks.iter().map(|&mark| (mark, true)).collect::<Vec<_>>();

    if ext.0.is_finite() {
        let pos = -ext.0;

        nodes.push((
            Mark {
                pos,
                dir: 1,
                bound: false,
            },
            false,
        ));
    }

    if ext.1.is_finite() {
        let pos = total + ext.1;

        nodes.push((
            Mark {
                pos,
                dir: -1,
                bound: false,
            },
            false,
        ));
    }

    // Pairing each opening mark with the next closing one spends every mark on a single stretch, so
    // close junctions never chain into one cross.
    nodes.sort_unstable_by(|(a, _), (b, _)| a.pos.total_cmp(&b.pos).then(a.dir.cmp(&b.dir)));

    let mut crosses = Vec::with_capacity(nodes.len());
    let mut bars = Vec::new();
    let mut i = 0;

    while i < nodes.len() {
        let (Mark { pos, dir, bound }, real) = nodes[i];

        if let Some(&(
            Mark {
                pos: next,
                dir: -1,
                bound: next_bound,
            },
            _,
        )) = nodes.get(i + 1)
            && dir == 1
            && next - pos < SHORT_SEGMENT
        {
            // A bar without its cross would not say which side it governs.
            if next - pos >= MIN_STRETCH {
                let mid = f64::midpoint(pos, next);

                // A stretch spanning ways is marked by the one its middle falls on.
                if (0.0..=total).contains(&mid) {
                    crosses.push(mid);
                }

                if bound {
                    bars.push(pos);
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
                bars.push(pos);
            }

            crosses.push(f64::from(dir).mul_add(MARK_OFFSET, pos).clamp(0.0, total));
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

    bars.sort_unstable_by(f64::total_cmp);
    bars.dedup_by(|a, b| (*a - *b).abs() < 0.5);

    (merged, bars)
}

/// Crosses repeated on a grid measured from the way's start, so that every tile places them alike,
/// each giving way to a mark closer than a short stretch, including those beyond the way's ends.
fn repeats(total: f64, marks: &[Mark], ext: (f64, f64)) -> Vec<f64> {
    let mut out = Vec::new();

    for step in 0.. {
        let pos = (f64::from(step) + 0.5) * REPEAT_SPACING;

        if pos >= total {
            break;
        }

        let i = marks.partition_point(|mark| mark.pos < pos);

        let nearest = [marks.get(i.wrapping_sub(1)), marks.get(i)]
            .into_iter()
            .flatten()
            .map(|mark| (mark.pos - pos).abs())
            .chain([pos + ext.0, total + ext.1 - pos])
            .fold(f64::INFINITY, f64::min);

        if nearest >= SHORT_SEGMENT {
            out.push(pos);
        }
    }

    out
}

/// Finds junctions from the roads' shared vertices, then places crosses and bars on every visible
/// drawn road.
fn plan(roads: &[Road], visible: impl Fn(&Road) -> bool) -> Vec<(usize, Vec<f64>, Vec<f64>)> {
    let drawn = roads.iter().filter(|road| road.draws);

    // Only the vertices of drawn roads are ever looked up.
    let mut index = NodeIndex::with_capacity(drawn.clone().map(|road| road.keys.len()).sum());

    for road in drawn {
        for &key in &road.keys {
            index.entry(key).or_default();
        }
    }

    for (r, road) in roads.iter().enumerate() {
        for (v, key) in road.keys.iter().enumerate() {
            if let Some(occurrences) = index.get_mut(key) {
                occurrences.push((r, v));
            }
        }
    }

    let topos = roads
        .iter()
        .enumerate()
        .map(|(r, road)| road.draws.then(|| topology(roads, &index, r)))
        .collect::<Vec<_>>();

    roads
        .iter()
        .enumerate()
        .filter(|(_, road)| road.draws && visible(road))
        .map(|(r, road)| {
            let topo = topos[r].as_ref().expect("a drawn road has its topology");
            let total = road.total();

            let ext = (
                reach(roads, &topos, topo.ends[0]),
                reach(roads, &topos, topo.ends[1]),
            );

            let (mut crosses, bars) = place_marks(&topo.marks, ext, total);

            crosses.extend(repeats(total, &topo.marks, ext));

            (r, crosses, bars)
        })
        .collect()
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("road_access_restrictions::render");

    let mut parsed = Vec::with_capacity(rows.len());

    for row in &rows {
        let map = row.get_line_string()?;

        let id = match row {
            Feature::Row(_) => Some(row.get_i64("osm_id")?),
            Feature::LegendData(_) => None,
        };

        let bits = restriction(
            row.get_string("access")?,
            row.get_string("vehicle")?,
            row.get_string("bicycle")?,
            row.get_string("foot")?,
        );

        let geom = map.project_to_tile(&ctx.tile_projector);

        if let Some(road) = Road::new(
            &map,
            geom,
            row.get_string("type")?,
            row.get_string("class")?,
            bits,
        ) {
            parsed.push((id, road));
        }
    }

    let roads = merge_rows(parsed);

    let width = f64::from(ctx.size.width);
    let height = f64::from(ctx.size.height);

    let visible = |road: &Road| {
        road.geom.bounding_rect().is_some_and(|rect| {
            rect.max().x >= -DRAW_BUFFER
                && rect.min().x <= width + DRAW_BUFFER
                && rect.max().y >= -DRAW_BUFFER
                && rect.min().y <= height + DRAW_BUFFER
        })
    };

    let placements = plan(&roads, visible);

    if placements.is_empty() {
        return Ok(());
    }

    let mut load = |name: &str| -> Result<_, SvgRepoError> {
        let surface = svg_repo.get(name)?.clone();
        let rect = surface.extents().expect("surface extents");

        Ok((surface, rect))
    };

    let no_bicycle = load("no_bicycle")?;
    let no_foot = load("no_foot")?;
    let no_foot_bicycle = load("no_foot_bicycle")?;
    let boundary = load("access_boundary")?;

    for (r, crosses, bars) in placements {
        let road = &roads[r];

        // Every cross shows the way's whole restriction; the bar alone says it changes right here.
        let cross = match road.bits {
            NO_BICYCLE => &no_bicycle,
            NO_FOOT => &no_foot,
            _ => &no_foot_bicycle,
        };

        let marks = crosses
            .into_iter()
            .map(|pos| (pos, cross))
            .chain(bars.into_iter().map(|pos| (pos, &boundary)));

        for (pos, (icon, rect)) in marks {
            let (x, y, angle) = sample(&road.geom, &road.lengths, pos);

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
    const BOTH: u8 = NO_BICYCLE | NO_FOOT;

    #[track_caller]
    fn assert_marks(got: &[f64], want: &[f64]) {
        assert_eq!(got.len(), want.len(), "got {got:?}, want {want:?}");

        for (got, want) in got.iter().zip(want) {
            assert!((got - want).abs() < 1e-9, "got {got:?}, want {want:?}");
        }
    }

    fn marks(list: &[(f64, i8)]) -> Vec<Mark> {
        list.iter()
            .map(|&(pos, dir)| Mark {
                pos,
                dir,
                bound: false,
            })
            .collect()
    }

    fn crosses(list: &[(f64, i8)], ext: (f64, f64), total: f64) -> Vec<f64> {
        place_marks(&marks(list), ext, total).0
    }

    fn road_of(coords: &[(f64, f64)], bits: u8, typ: &str, class: &str) -> Road {
        let line = LineString::from(coords.to_vec());

        Road::new(&line, line.clone(), typ, class, bits).expect("a valid road")
    }

    fn road(coords: &[(f64, f64)], bits: u8) -> Road {
        road_of(coords, bits, "path", "highway")
    }

    /// Crosses (sorted) and bars of every road.
    fn placed(roads: &[Road]) -> Vec<(Vec<f64>, Vec<f64>)> {
        let mut out = vec![(Vec::new(), Vec::new()); roads.len()];

        for (r, mut crosses, bars) in plan(roads, |_| true) {
            crosses.sort_unstable_by(f64::total_cmp);
            out[r] = (crosses, bars);
        }

        out
    }

    #[test]
    fn restriction_follows_the_access_tags() {
        assert_eq!(restriction("no", "", "", ""), BOTH);
        assert_eq!(restriction("", "", "no", ""), NO_BICYCLE);
        assert_eq!(restriction("", "", "", "no"), NO_FOOT);
        assert_eq!(restriction("no", "", "", "yes"), NO_BICYCLE);
        assert_eq!(restriction("yes", "no", "", ""), NO_BICYCLE);
        assert_eq!(restriction("private", "", "designated", ""), NO_FOOT);
        assert_eq!(restriction("", "", "customers", ""), NO_BICYCLE);
        assert_eq!(restriction("", "", "", ""), 0);
    }

    #[test]
    fn short_stretch_collapses_to_one_cross_in_the_middle() {
        assert_marks(&crosses(&[(30.0, 1), (75.0, -1)], OPEN, 300.0), &[52.5]);
    }

    #[test]
    fn long_stretch_keeps_a_cross_at_each_end() {
        assert_marks(
            &crosses(&[(30.0, 1), (225.0, -1)], OPEN, 300.0),
            &[40.0, 215.0],
        );
    }

    #[test]
    fn collapsing_does_not_chain_across_a_run_of_close_junctions() {
        let list = [
            (30.0, -1),
            (30.0, 1),
            (60.0, -1),
            (60.0, 1),
            (90.0, -1),
            (90.0, 1),
            (120.0, -1),
            (120.0, 1),
        ];

        assert_marks(
            &crosses(&list, OPEN, 300.0),
            &[20.0, 45.0, 75.0, 105.0, 130.0],
        );
    }

    #[test]
    fn a_stretch_too_short_to_mark_gets_neither_cross_nor_bar() {
        let mark = |len: f64| {
            let mut list = marks(&[(30.0, 1), (30.0 + len, -1)]);

            list[0].bound = true;

            place_marks(&list, OPEN, 300.0)
        };

        let (crosses, bars) = mark(MIN_STRETCH - 1.0);

        assert!(
            crosses.is_empty() && bars.is_empty(),
            "{crosses:?} {bars:?}"
        );

        let (crosses, bars) = mark(MIN_STRETCH + 1.0);

        assert_eq!((crosses.len(), bars.len()), (1, 1));
    }

    #[test]
    fn a_stretch_just_over_the_threshold_still_gets_both_ends() {
        let over = SHORT_SEGMENT + 1.0;
        let under = SHORT_SEGMENT - 1.0;

        assert_eq!(crosses(&[(0.0, 1), (over, -1)], OPEN, 300.0).len(), 2);
        assert_eq!(crosses(&[(0.0, 1), (under, -1)], OPEN, 300.0).len(), 1);
    }

    #[test]
    fn a_stretch_leaving_the_way_keeps_its_offset_cross() {
        assert_marks(&crosses(&[(100.0, -1)], OPEN, 200.0), &[90.0]);
    }

    #[test]
    fn a_stretch_spanning_two_ways_is_marked_once_by_the_way_holding_its_middle() {
        let on_a = crosses(&[(90.0, 1)], (f64::INFINITY, 30.0), 100.0);
        let on_b = crosses(&[(30.0, -1)], (10.0, f64::INFINITY), 30.0);

        assert_marks(&on_a, &[]);
        assert_marks(&on_b, &[10.0]);
    }

    #[test]
    fn repeats_sit_on_a_grid_and_give_way_to_near_marks() {
        assert_marks(&repeats(1000.0, &[], OPEN), &[150.0, 450.0, 750.0]);
        assert_marks(
            &repeats(1000.0, &marks(&[(0.0, 1), (280.0, -1)]), OPEN),
            &[450.0, 750.0],
        );
        assert_marks(&repeats(200.0, &[], (f64::INFINITY, 20.0)), &[]);
    }

    #[test]
    fn every_arm_of_a_junction_is_marked() {
        let roads = [
            road(&[(0.0, 0.0), (200.0, 0.0)], BOTH),
            road(&[(0.0, 0.0), (0.0, 200.0)], BOTH),
            road(&[(0.0, 0.0), (-200.0, 0.0)], BOTH),
        ];

        for (crosses, bars) in placed(&roads) {
            assert_marks(&crosses, &[10.0, 190.0]);
            assert!(bars.is_empty());
        }
    }

    #[test]
    fn a_way_passing_through_a_junction_is_marked_on_both_sides() {
        let roads = [
            road(&[(-200.0, 0.0), (0.0, 0.0), (200.0, 0.0)], BOTH),
            road(&[(0.0, 0.0), (0.0, 200.0)], BOTH),
        ];

        let placed = placed(&roads);

        assert_marks(&placed[0].0, &[10.0, 190.0, 210.0, 390.0]);
        assert_marks(&placed[1].0, &[10.0, 190.0]);
    }

    #[test]
    fn a_long_stretch_over_two_ways_keeps_its_offset_crosses() {
        let roads = [
            road(&[(0.0, 0.0), (100.0, 0.0)], BOTH),
            road(&[(100.0, 0.0), (200.0, 0.0)], BOTH),
        ];

        let placed = placed(&roads);

        assert_marks(&placed[0].0, &[10.0]);
        assert_marks(&placed[1].0, &[90.0]);
    }

    #[test]
    fn a_short_stretch_over_two_ways_gets_one_cross_on_the_way_holding_its_middle() {
        let roads = [
            road(&[(0.0, 0.0), (60.0, 0.0)], BOTH),
            road(&[(60.0, 0.0), (100.0, 0.0)], BOTH),
        ];

        let placed = placed(&roads);

        assert_marks(&placed[0].0, &[50.0]);
        assert_marks(&placed[1].0, &[]);
    }

    #[test]
    fn a_restriction_starting_without_a_junction_gets_a_bar() {
        let roads = [
            road(&[(0.0, 0.0), (200.0, 0.0)], NO_FOOT),
            road(&[(200.0, 0.0), (400.0, 0.0)], 0),
        ];

        let placed = placed(&roads);

        assert_marks(&placed[0].0, &[10.0, 190.0]);
        assert_marks(&placed[0].1, &[200.0]);
    }

    #[test]
    fn a_stretch_ends_where_a_stricter_way_takes_over() {
        let roads = [
            road(&[(0.0, 0.0), (40.0, 0.0)], NO_BICYCLE),
            road(&[(40.0, 0.0), (200.0, 0.0)], BOTH),
        ];

        let placed = placed(&roads);

        assert_marks(&placed[0].0, &[20.0]);
        assert_marks(&placed[1].0, &[10.0, 150.0]);
        assert_marks(&placed[1].1, &[0.0]);
    }

    #[test]
    fn a_p_shaped_way_is_a_junction_where_its_end_meets_itself() {
        let roads = [road(
            &[
                (0.0, -100.0),
                (0.0, 0.0),
                (100.0, 0.0),
                (100.0, 100.0),
                (0.0, 100.0),
                (0.0, 0.0),
            ],
            BOTH,
        )];

        assert_marks(&placed(&roads)[0].0, &[50.0, 110.0, 490.0]);
    }

    #[test]
    fn both_arms_of_a_loop_are_marked_at_a_junction_on_its_seam() {
        let roads = [
            road(
                &[
                    (0.0, 0.0),
                    (200.0, 0.0),
                    (200.0, 200.0),
                    (0.0, 200.0),
                    (0.0, 0.0),
                ],
                BOTH,
            ),
            road(&[(0.0, 0.0), (0.0, -200.0)], 0),
        ];

        assert_marks(&placed(&roads)[0].0, &[10.0, 150.0, 450.0, 790.0]);
    }

    #[test]
    fn a_short_stretch_across_the_seam_of_a_loop_gets_one_cross() {
        let roads = [
            road(
                &[
                    (0.0, 0.0),
                    (30.0, 0.0),
                    (30.0, 30.0),
                    (0.0, 30.0),
                    (0.0, 0.0),
                ],
                BOTH,
            ),
            road(&[(30.0, 0.0), (30.0, -200.0)], 0),
        ];

        assert_marks(&placed(&roads)[0].0, &[90.0]);
    }

    #[test]
    fn a_stretch_running_onto_an_undrawn_trunk_counts_as_long() {
        let roads = [
            road(&[(0.0, 0.0), (60.0, 0.0)], BOTH),
            road_of(&[(60.0, 0.0), (100.0, 0.0)], BOTH, "trunk", "highway"),
        ];

        assert_marks(&placed(&roads)[0].0, &[10.0]);
    }

    #[test]
    fn a_railway_is_no_junction() {
        let roads = [
            road(&[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)], BOTH),
            road_of(
                &[(100.0, -100.0), (100.0, 0.0), (100.0, 100.0)],
                0,
                "rail",
                "railway",
            ),
        ];

        assert_marks(&placed(&roads)[0].0, &[10.0, 190.0]);
    }

    #[test]
    fn rows_of_one_way_merge_into_one_road() {
        let coords = [(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)];

        let roads = merge_rows(vec![
            (Some(7), road_of(&coords, NO_BICYCLE, "tram", "railway")),
            (Some(7), road_of(&coords, NO_BICYCLE, "service", "highway")),
        ]);

        assert_eq!(roads.len(), 1);
        assert!(roads[0].joins && roads[0].draws);
        assert_marks(&placed(&roads)[0].0, &[10.0, 190.0]);
    }

    #[test]
    fn a_neighbouring_way_stored_as_two_rows_counts_once() {
        let pier = [(100.0, 0.0), (200.0, 0.0)];

        let roads = merge_rows(vec![
            (None, road(&[(0.0, 0.0), (100.0, 0.0)], BOTH)),
            (Some(5), road_of(&pier, 0, "footway", "highway")),
            (Some(5), road_of(&pier, 0, "pier", "man_made")),
        ]);

        let placed = placed(&roads);

        // Counted twice, the pier would make a junction of the node and the bar would be lost.
        assert_marks(&placed[0].0, &[50.0]);
        assert_marks(&placed[0].1, &[100.0]);
    }
}
