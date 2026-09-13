use crate::render::{
    Feature,
    ctx::Ctx,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::{SvgRepo, SvgRepoError},
};
use cairo::{Context, RecordingSurface, Rectangle};
use geo::{BoundingRect, Coord, LineString};
use std::{collections::HashMap, f64::consts::PI};

const NO_BICYCLE: u8 = 1;
const NO_FOOT: u8 = 2;

/// Keeps a mark off its junction, where it would read as speaking for the joining way too.
const MARK_OFFSET: f64 = 10.0;

/// Marks of one kind closer together than this collapse into one.
const MERGE_GAP: f64 = 10.0;

/// Gaps between marks are halved until no piece is longer than this.
const MAX_GAP: f64 = 300.0;

/// A stretch shorter than this gets one mark in its middle.
const SHORT_SEGMENT: f64 = MAX_GAP / 2.0;

/// A stretch shorter than this gets no mark, which would cover the junctions at both its ends.
const MIN_STRETCH: f64 = 2.0 * MARK_OFFSET;

/// Distance between a cross and an arrow that would otherwise overlap.
const SIDE_BY_SIDE: f64 = 10.0;

/// How far outside the tile a mark can still show.
const DRAW_BUFFER: f64 = 32.0;

/// Longest gap halved over its full length: both its ends must lie within the fetched roads.
const HALVED_GAP: f64 = 500.0;

/// Covers every node a visible mark can depend on, so each gets its complete degree.
const TOPOLOGY_BUFFER: f64 = HALVED_GAP + DRAW_BUFFER + SIDE_BY_SIDE + MERGE_GAP + MARK_OFFSET;

const UNDRAWN_TYPES: [&str; 4] = ["trunk", "motorway", "trunk_link", "motorway_link"];

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT osm_id, geometry, type, class, access, vehicle, bicycle, foot, oneway
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
    /// `1` travelled along the geometry, `-1` against it, `0` both ways.
    oneway: i16,
    /// Railways never count as a way at a node.
    joins: bool,
    /// Carries restriction marks.
    restricted: bool,
}

impl Road {
    fn new(
        map: &LineString,
        geom: LineString,
        typ: &str,
        class: &str,
        bits: u8,
        oneway: i16,
    ) -> Option<Self> {
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
            oneway: oneway.signum(),
            joins: class != "railway",
            restricted: bits != 0 && !UNDRAWN_TYPES.contains(&typ),
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

    /// Whether travel along this way heads into `vertex`, one of its ends.
    const fn enters(&self, vertex: usize) -> bool {
        (self.oneway > 0 && vertex == self.last()) || (self.oneway < 0 && vertex == 0)
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
            prev.restricted |= road.restricted;

            continue;
        }

        last_id = id;
        roads.push(road);
    }

    roads
}

/// What a way's marks say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// Crosses for what is forbidden, bars where that changes without a junction.
    Access,
    /// Arrows for the direction of travel.
    Oneway,
}

/// How a stretch ending at a plain continuation meets the way it runs into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Meeting {
    Same,
    /// The stretch ends and this way marks the node.
    Boundary,
    /// The stretch ends and the other way marks the node.
    Yield,
}

impl Kind {
    const fn draws(self, road: &Road) -> bool {
        match self {
            Self::Access => road.restricted,
            Self::Oneway => road.oneway != 0,
        }
    }

    const fn meets(self, road: &Road, vertex: usize, other: &Road, other_vertex: usize) -> Meeting {
        match self {
            Self::Access if road.bits & !other.bits != 0 => Meeting::Boundary,
            Self::Access if other.bits & !road.bits != 0 => Meeting::Yield,
            Self::Access => Meeting::Same,
            // Travel carries on only if it enters the node on one way and leaves it on the other.
            Self::Oneway
                if other.oneway != 0 && road.enters(vertex) != other.enters(other_vertex) =>
            {
                Meeting::Same
            }
            Self::Oneway => Meeting::Boundary,
        }
    }
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
    /// The way it runs into marks the node itself.
    Yield,
    /// The stretch runs on into this vertex, an end of a way it carries on along.
    Continues(usize, usize),
}

struct Topology {
    /// Ascending by position.
    marks: Vec<Mark>,
    ends: [End; 2],
}

fn topology(kind: Kind, roads: &[Road], index: &NodeIndex, r: usize) -> Topology {
    let road = &roads[r];
    let mut marks = Vec::new();
    let mut ends = [End::Marked; 2];

    for (v, key) in road.keys.iter().enumerate() {
        let mut degree = 0;
        let mut other = None;

        for &(o, ov) in &index[key] {
            if o != r && !roads[o].joins {
                continue;
            }

            degree += roads[o].arms(ov);

            if o != r {
                other = Some((o, ov));
            }
        }

        let meeting = match other {
            Some((o, ov)) if degree == 2 => kind.meets(road, v, &roads[o], ov),
            _ => Meeting::Same,
        };

        // A continuation is a stretch boundary only where what the marks say changes.
        let is_mark = degree != 2 || meeting == Meeting::Boundary;
        let pos = road.lengths[v];

        if is_mark {
            // Only a continuation hides where a restriction changes, so only there does a bar show it.
            let bound = kind == Kind::Access && degree == 2;

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
            } else if meeting == Meeting::Yield {
                End::Yield
            } else {
                // Nothing else here means the seam of a loop, which runs on into its other end.
                let (o, ov) = other.unwrap_or_else(|| (r, road.last() - v));

                End::Continues(o, ov)
            };
        }
    }

    Topology { marks, ends }
}

/// How far a stretch runs on past a way's end before a mark bounds it; infinite once that reaches
/// [`HALVED_GAP`], or when it runs onto a way that never draws its marks.
fn reach(roads: &[Road], topos: &[Option<Topology>], mut end: End) -> f64 {
    let mut acc = 0.0;

    // Bounds a loop of mark-less ways.
    for _ in 0..=roads.len() {
        let (road, vertex) = match end {
            End::Marked => return f64::INFINITY,
            End::Yield => return acc,
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

        if acc >= HALVED_GAP {
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

/// Symbols and bars for a way's marks, given how far its stretches run on past its start and end
/// (`ext`): a short stretch gets one symbol in its middle, and one too short for any gets nothing.
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
    // close junctions never chain into one symbol.
    nodes.sort_unstable_by(|(a, _), (b, _)| a.pos.total_cmp(&b.pos).then(a.dir.cmp(&b.dir)));

    let mut symbols = Vec::with_capacity(nodes.len());
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
                    symbols.push(mid);
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

            symbols.push(f64::from(dir).mul_add(MARK_OFFSET, pos).clamp(0.0, total));
        }

        i += 1;
    }

    symbols.sort_unstable_by(f64::total_cmp);

    let mut merged = Vec::<f64>::with_capacity(symbols.len());

    for pos in symbols {
        match merged.last_mut() {
            Some(last) if pos - *last < MERGE_GAP => *last = f64::midpoint(*last, pos),
            _ => merged.push(pos),
        }
    }

    bars.sort_unstable_by(f64::total_cmp);
    bars.dedup_by(|a, b| (*a - *b).abs() < 0.5);

    (merged, bars)
}

/// Halves every gap between marks, those beyond the way's ends included, into pieces of at most
/// [`MAX_GAP`]. A gap of [`HALVED_GAP`] or more may end beyond the fetched roads, so it takes its own
/// way's halves instead, which every tile knows alike; a way under [`SHORT_SEGMENT`] takes none.
fn halve(total: f64, marks: &[Mark], ext: (f64, f64)) -> Vec<f64> {
    let mut bounds = std::iter::once(-ext.0)
        .chain(marks.iter().map(|mark| mark.pos))
        .chain(std::iter::once(total + ext.1))
        .collect::<Vec<_>>();

    bounds.dedup();

    let mut own = Vec::new();

    if total >= SHORT_SEGMENT {
        own.push(total / 2.0);
        split(0.0, total / 2.0, &mut own);
        split(total / 2.0, total, &mut own);
    }

    let mut out = Vec::new();

    for gap in bounds.windows(2) {
        let (from, to) = (gap[0], gap[1]);

        if to - from < HALVED_GAP {
            split(from, to, &mut out);
        } else {
            out.extend(own.iter().filter(|&&pos| {
                from + SHORT_SEGMENT / 2.0 < pos && pos < to - SHORT_SEGMENT / 2.0
            }));
        }
    }

    out.retain(|pos| (0.0..=total).contains(pos));
    out.sort_unstable_by(f64::total_cmp);

    out
}

fn split(from: f64, to: f64, out: &mut Vec<f64>) {
    if to - from <= MAX_GAP {
        return;
    }

    let mid = f64::midpoint(from, to);

    out.push(mid);

    split(from, mid, out);
    split(mid, to, out);
}

/// Every road's vertices that some drawing road passes through, the only ones ever looked up.
fn node_index(roads: &[Road]) -> NodeIndex {
    let drawing = roads
        .iter()
        .filter(|road| Kind::Access.draws(road) || Kind::Oneway.draws(road));

    let mut index = NodeIndex::with_capacity(drawing.clone().map(|road| road.keys.len()).sum());

    for road in drawing {
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

    index
}

/// Sorted symbols and bars of `kind` for each road, empty where it draws none.
fn plan(
    kind: Kind,
    roads: &[Road],
    index: &NodeIndex,
    visible: impl Fn(&Road) -> bool,
) -> Vec<(Vec<f64>, Vec<f64>)> {
    let topos = roads
        .iter()
        .enumerate()
        .map(|(r, road)| kind.draws(road).then(|| topology(kind, roads, index, r)))
        .collect::<Vec<_>>();

    roads
        .iter()
        .zip(&topos)
        .map(|(road, topo)| {
            let Some(topo) = topo.as_ref().filter(|_| visible(road)) else {
                return Default::default();
            };

            let total = road.total();

            let ext = (
                reach(roads, &topos, topo.ends[0]),
                reach(roads, &topos, topo.ends[1]),
            );

            let (mut symbols, bars) = place_marks(&topo.marks, ext, total);

            symbols.extend(halve(total, &topo.marks, ext));
            symbols.sort_unstable_by(f64::total_cmp);

            (symbols, bars)
        })
        .collect()
}

/// Moves a cross and an arrow that would overlap apart, the cross first in the direction of travel.
/// Both lists must be sorted.
fn side_by_side(crosses: &mut [f64], arrows: &mut [f64], oneway: i16, total: f64) {
    let half = f64::from(oneway) * SIDE_BY_SIDE / 2.0;
    let (mut c, mut a) = (0, 0);

    while c < crosses.len() && a < arrows.len() {
        if (crosses[c] - arrows[a]).abs() < SIDE_BY_SIDE {
            let mid = f64::midpoint(crosses[c], arrows[a]);

            crosses[c] = (mid - half).clamp(0.0, total);
            arrows[a] = (mid + half).clamp(0.0, total);

            c += 1;
            a += 1;
        } else if crosses[c] < arrows[a] {
            c += 1;
        } else {
            a += 1;
        }
    }
}

fn paint(
    context: &Context,
    road: &Road,
    pos: f64,
    turn: f64,
    (icon, rect): &(RecordingSurface, Rectangle),
    alpha: f64,
) -> cairo::Result<()> {
    let (x, y, angle) = sample(&road.geom, &road.lengths, pos);

    context.save()?;
    context.translate(x, y);
    context.rotate(angle + turn);
    context.set_source_surface(icon, -rect.width() / 2.0, -rect.height() / 2.0)?;
    context.paint_with_alpha(alpha)?;
    context.restore()
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
            row.get_i16("oneway")?,
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

    let index = node_index(&roads);

    let access = plan(Kind::Access, &roads, &index, visible);
    let oneway = plan(Kind::Oneway, &roads, &index, visible);

    if access
        .iter()
        .chain(&oneway)
        .all(|(symbols, bars)| symbols.is_empty() && bars.is_empty())
    {
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
    let arrow = load("highway-arrow")?;

    for ((road, (mut crosses, bars)), (mut arrows, _)) in roads.iter().zip(access).zip(oneway) {
        side_by_side(&mut crosses, &mut arrows, road.oneway, road.total());

        // Every cross shows the way's whole restriction; the bar alone says it changes right here.
        let cross = match road.bits {
            NO_BICYCLE => &no_bicycle,
            NO_FOOT => &no_foot,
            _ => &no_foot_bicycle,
        };

        for pos in crosses {
            paint(context, road, pos, 0.0, cross, 0.75)?;
        }

        for pos in bars {
            paint(context, road, pos, 0.0, &boundary, 0.75)?;
        }

        let turn = if road.oneway < 0 { PI } else { 0.0 };

        for pos in arrows {
            paint(context, road, pos, turn, &arrow, 1.0)?;
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

    fn road_with(coords: &[(f64, f64)], bits: u8, oneway: i16, typ: &str, class: &str) -> Road {
        let line = LineString::from(coords.to_vec());

        Road::new(&line, line.clone(), typ, class, bits, oneway).expect("a valid road")
    }

    fn road_of(coords: &[(f64, f64)], bits: u8, typ: &str, class: &str) -> Road {
        road_with(coords, bits, 0, typ, class)
    }

    fn road(coords: &[(f64, f64)], bits: u8) -> Road {
        road_of(coords, bits, "path", "highway")
    }

    fn oneway(coords: &[(f64, f64)], oneway: i16) -> Road {
        road_with(coords, 0, oneway, "residential", "highway")
    }

    fn placed_as(kind: Kind, roads: &[Road]) -> Vec<(Vec<f64>, Vec<f64>)> {
        plan(kind, roads, &node_index(roads), |_| true)
    }

    fn placed(roads: &[Road]) -> Vec<(Vec<f64>, Vec<f64>)> {
        placed_as(Kind::Access, roads)
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
    fn gaps_are_halved_and_halved_again() {
        let between = |to: f64| halve(to, &marks(&[(0.0, 1), (to, -1)]), OPEN);

        assert_marks(&between(300.0), &[]);
        assert_marks(&between(400.0), &[200.0]);
        assert_marks(&between(1000.0), &[250.0, 500.0, 750.0]);
    }

    #[test]
    fn a_gap_spanning_ways_is_halved_alike_on_both() {
        let on_a = halve(200.0, &marks(&[(0.0, 1)]), (f64::INFINITY, 250.0));
        let on_b = halve(250.0, &marks(&[(250.0, -1)]), (200.0, f64::INFINITY));

        assert_marks(&on_a, &[]);
        assert_marks(&on_b, &[25.0]);
    }

    #[test]
    fn a_gap_too_long_to_know_whole_takes_the_halves_of_its_way() {
        let ends = marks(&[(100.0, 1), (1000.0, -1)]);

        assert_marks(&halve(1000.0, &ends, OPEN), &[250.0, 500.0, 750.0]);
        assert_marks(&halve(250.0, &[], OPEN), &[125.0]);
        assert_marks(&halve(100.0, &[], OPEN), &[]);
    }

    #[test]
    fn a_long_chain_of_short_ways_is_marked_on_each() {
        let ways = (0..5)
            .map(|i| {
                let x = f64::from(i) * 250.0;

                oneway(&[(x, 0.0), (x + 250.0, 0.0)], 1)
            })
            .collect::<Vec<_>>();

        let placed = placed_as(Kind::Oneway, &ways);

        assert_marks(&placed[0].0, &[10.0, 125.0]);
        assert_marks(&placed[2].0, &[125.0]);
        assert_marks(&placed[4].0, &[125.0, 240.0]);
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

        assert_marks(&placed(&roads)[0].0, &[50.0, 110.0, 300.0, 490.0]);
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

        assert_marks(&placed(&roads)[0].0, &[10.0, 200.0, 400.0, 600.0, 790.0]);
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
        assert!(roads[0].joins && roads[0].restricted);
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

    #[test]
    fn a_oneway_way_gets_arrows_near_its_ends_and_no_bars() {
        let placed = placed_as(Kind::Oneway, &[oneway(&[(0.0, 0.0), (200.0, 0.0)], 1)]);

        assert_marks(&placed[0].0, &[10.0, 190.0]);
        assert!(placed[0].1.is_empty());
    }

    #[test]
    fn oneway_travel_carries_on_into_a_way_drawn_the_other_way_round() {
        let roads = [
            oneway(&[(0.0, 0.0), (60.0, 0.0)], 1),
            oneway(&[(100.0, 0.0), (60.0, 0.0)], -1),
        ];

        let placed = placed_as(Kind::Oneway, &roads);

        assert_marks(&placed[0].0, &[50.0]);
        assert_marks(&placed[1].0, &[]);
    }

    #[test]
    fn oneway_ways_meeting_head_on_are_separate_stretches() {
        let roads = [
            oneway(&[(0.0, 0.0), (100.0, 0.0)], 1),
            oneway(&[(200.0, 0.0), (100.0, 0.0)], 1),
        ];

        let placed = placed_as(Kind::Oneway, &roads);

        assert_marks(&placed[0].0, &[50.0]);
        assert_marks(&placed[1].0, &[50.0]);
    }

    #[test]
    fn a_way_that_stops_being_oneway_ends_the_stretch() {
        let roads = [
            oneway(&[(0.0, 0.0), (200.0, 0.0)], 1),
            oneway(&[(200.0, 0.0), (400.0, 0.0)], 0),
        ];

        let placed = placed_as(Kind::Oneway, &roads);

        assert_marks(&placed[0].0, &[10.0, 190.0]);
        assert_marks(&placed[1].0, &[]);
    }

    #[test]
    fn an_arrow_and_a_cross_on_one_spot_sit_side_by_side_cross_first() {
        let (mut crosses, mut arrows) = (vec![10.0], vec![10.0]);

        side_by_side(&mut crosses, &mut arrows, 1, 200.0);

        assert_marks(&crosses, &[5.0]);
        assert_marks(&arrows, &[15.0]);

        let (mut crosses, mut arrows) = (vec![10.0], vec![10.0]);

        side_by_side(&mut crosses, &mut arrows, -1, 200.0);

        assert_marks(&crosses, &[15.0]);
        assert_marks(&arrows, &[5.0]);

        let (mut crosses, mut arrows) = (vec![10.0], vec![50.0]);

        side_by_side(&mut crosses, &mut arrows, 1, 200.0);

        assert_marks(&crosses, &[10.0]);
        assert_marks(&arrows, &[50.0]);
    }
}
