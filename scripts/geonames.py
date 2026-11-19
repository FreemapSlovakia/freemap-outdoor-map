"""Build label lines with zoom ranges for mountain ranges and geomorphological units.

Usage: python3 scripts/geonames.py <osmium-export.geojsonseq> <out.geojson> [limit.geojson]
"""

import json
import math
import sys

import networkx as nx
import numpy as np
import shapely
from shapely.geometry import LineString, mapping, shape
from shapely.ops import substring
from shapely.prepared import prep

R = 6378137.0

# Label line length in pixels at which a unit hands over to its children.
MAX_PX = 600
# Shortest label line worth showing; a unit far smaller than its parent waits for it.
MIN_PX = 80

# Smallest part of a multipolygon unit that gets its own name, as a share of the largest.
PART_MIN_SHARE = 0.2

# Landforms labelled like mountain ranges; natural=depression is left out, as in OSM it is
# mostly small craters.
NATURAL = {"mountain_range", "plateau", "plain", "massif", "hills"}


def to_merc(g):
    def f(c):
        return np.column_stack(
            [R * np.radians(c[:, 0]), R * np.log(np.tan(np.pi / 4 + np.radians(c[:, 1]) / 2))]
        )

    return shapely.transform(g, f)


def to_wgs(g):
    def f(c):
        return np.column_stack(
            [np.degrees(c[:, 0] / R), np.degrees(2 * np.arctan(np.exp(c[:, 1] / R)) - np.pi / 2)]
        )

    return shapely.transform(g, f)


def largest(g):
    polys = [p for p in shapely.get_parts(g) if p.geom_type == "Polygon" and not p.is_empty]
    return max(polys, key=lambda p: p.area) if polys else None


def spine(poly, width):
    """Path through the Voronoi skeleton of the polygon to label it along."""
    pts = np.vstack(
        [shapely.get_coordinates(shapely.segmentize(r, width / 15)) for r in [poly.exterior, *poly.interiors]]
    )
    edges = shapely.voronoi_polygons(shapely.MultiPoint(pts), only_edges=True)
    prepared = prep(poly)

    g = nx.Graph()
    for e in edges.geoms:
        if prepared.contains(e):
            a, b = (tuple(round(c, 1) for c in p) for p in e.coords)
            g.add_edge(a, b, weight=math.dist(a, b))

    if g.number_of_edges() == 0:
        return None
    g = g.subgraph(max(nx.connected_components(g), key=len))

    # Connect the two tips farthest apart as the crow flies, not along the skeleton: the
    # longest skeleton path likes to wander into a side arm and double back.
    tips = [n for n in g if g.degree(n) == 1]
    if len(tips) < 2:
        return None
    # The farthest pair lies on the convex hull, which keeps the pairwise search small.
    hull = set(map(tuple, shapely.get_coordinates(shapely.MultiPoint(tips).convex_hull)))
    pts = np.array([t for t in tips if t in hull] or tips)
    d = np.linalg.norm(pts[:, None] - pts[None], axis=-1)
    a, b = np.unravel_index(d.argmax(), d.shape)
    return LineString(nx.dijkstra_path(g, tuple(pts[a]), tuple(pts[b])))


def smooth(line, width):
    """Gaussian-smooth the spine so bends tighter than the unit's width average out."""
    n = max(int(line.length / (width / 10)), 2)
    pts = shapely.get_coordinates(shapely.line_interpolate_point(line, np.linspace(0, line.length, n + 1)))
    sigma = 5.0  # samples, i.e. half the width
    k = min(int(3 * sigma), len(pts) - 1)
    kernel = np.exp(-0.5 * (np.arange(-k, k + 1) / sigma) ** 2)
    kernel /= kernel.sum()
    # Point reflection about each end keeps the endpoints and their direction in place.
    padded = np.vstack([2 * pts[0] - pts[k:0:-1], pts, 2 * pts[-1] - pts[-2 : -k - 2 : -1]])
    return LineString(np.column_stack([np.convolve(padded[:, i], kernel, "valid") for i in (0, 1)]))


def extend(line, poly, width):
    """Run each end straight on along its direction up to the outline, less a margin."""
    margin = width * 0.2
    c = list(line.coords)
    ends = (
        (len(c), c[-1], line.interpolate(max(line.length - width * 0.5, line.length * 0.7))),
        (0, c[0], line.interpolate(min(width * 0.5, line.length * 0.3))),
    )
    for at, end, back in ends:
        dx, dy = end[0] - back.x, end[1] - back.y
        n = math.hypot(dx, dy)
        if n == 0:
            continue
        ray = LineString([end, (end[0] + dx / n * 1e7, end[1] + dy / n * 1e7)])
        hit = ray.intersection(poly.boundary)
        if hit.is_empty:
            continue
        t = min(math.dist(end, p) for p in shapely.get_coordinates(hit)) - margin
        if t > 0:
            c.insert(at, (end[0] + dx / n * t, end[1] + dy / n * t))
    return LineString(c)


def left_to_right(line):
    return line.reverse() if line.coords[0][0] > line.coords[-1][0] else line


def label_line(poly):
    centre, edge = shapely.maximum_inscribed_circle(poly, poly.length / 10000).coords
    width = 2 * math.dist(centre, edge)

    # Smooth the outline first: a jagged boundary sprouts a skeleton branch per notch.
    # No inward buffer: on a wide unit it cuts off narrow lobes the label should reach.
    core = largest(poly.simplify(width * 0.05))
    line = spine(core, width) if core else None

    if line is None or line.length < width * 1.5:
        # Compact unit: a skeleton spine is arbitrary, so take the longest straight chord
        # through the widest spot, less a small margin at each end.
        def chord(a):
            reach = []
            for sign in (1, -1):
                ray = LineString([centre, (centre[0] + sign * math.cos(a) * 1e7, centre[1] + sign * math.sin(a) * 1e7)])
                hit = shapely.get_coordinates(ray.intersection(poly.boundary))
                reach.append(min(math.dist(centre, p) for p in hit) - width * 0.05 if len(hit) else 0)
            return LineString([
                (centre[0] - math.cos(a) * reach[1], centre[1] - math.sin(a) * reach[1]),
                (centre[0] + math.cos(a) * reach[0], centre[1] + math.sin(a) * reach[0]),
            ])

        line = max((chord(math.radians(a)) for a in range(0, 180, 10)), key=lambda c: c.length)
        return left_to_right(line), "compact"

    # Within about half a width of a tip the skeleton forks into the corners; cut that off
    # before smoothing, which would otherwise keep the fork's direction as a curl.
    trim = min(line.length * 0.15, width * 0.5)
    line = smooth(substring(line, trim, line.length - trim), width).simplify(width * 0.03)

    return left_to_right(extend(line, poly, width)), "spine"


def read(src, limit):
    if limit:
        with open(limit) as fh:
            area = shapely.union_all([shape(f["geometry"]) for f in json.load(fh)["features"]])
        area = prep(to_merc(area))

    areas, mapped = [], []
    with open(src) as fh:
        for raw in fh:
            f = json.loads(raw.strip("\x1e"))
            p = f["properties"]
            if not p.get("name") or (
                p.get("boundary") != "geomorphological-unit"
                and p.get("natural") not in NATURAL
                and p.get("region:type") != "mountain_area"
            ):
                continue
            geom = to_merc(shape(f["geometry"]))
            if limit and not area.intersects(geom):
                continue
            if geom.geom_type == "LineString":
                mapped.append((p, geom))
            elif geom.geom_type in ("Polygon", "MultiPolygon"):
                areas.append((p, shapely.make_valid(geom)))
    return areas, mapped


def main(src, dst, limit=None):
    areas, mapped = read(src, limit)
    tree = shapely.STRtree([g for _, g in areas])

    # Parent = smallest area covering most of a feature; works without relation nesting.
    def find_parent(geom, i=None):
        if geom.geom_type == "LineString":
            covered = lambda j: areas[j][1].intersection(geom).length > 0.8 * geom.length  # noqa: E731
        else:
            # TODO: two areas with the same outline never nest and both get labelled.
            covered = lambda j: (  # noqa: E731
                areas[j][1].area > geom.area and areas[j][1].intersection(geom).area > 0.8 * geom.area
            )
        cands = [j for j in tree.query(geom) if j != i and covered(j)]
        return min(cands, key=lambda j: areas[j][1].area) if cands else None

    area_parent = [find_parent(geom, i) for i, (_, geom) in enumerate(areas)]

    labels = []  # (properties, line, method, parent area index)
    line_of = {}  # area index -> index into labels
    part_of = {}  # label index of a further part -> label index of the unit's main part
    for i, (p, geom) in enumerate(areas):
        parts = sorted(
            (q for q in shapely.get_parts(geom) if q.geom_type == "Polygon" and not q.is_empty),
            key=lambda q: -q.area,
        )
        # A unit split into comparable parts gets a name on each; slivers get none.
        for poly in (q for q in parts if q.area >= parts[0].area * PART_MIN_SHARE):
            try:
                line, method = label_line(poly)
            except Exception as e:  # noqa: BLE001
                print(f"{p['name']}: {e}", file=sys.stderr)
                continue
            if line.length > 0:
                if i in line_of:
                    part_of[len(labels)] = line_of[i]
                else:
                    line_of[i] = len(labels)
                labels.append((p, line, method, area_parent[i]))

    for p, line in mapped:
        # The same range is often mapped both as an area and as a line; the area wins.
        same = (j for j in tree.query(line, predicate="intersects") if areas[j][0]["name"] == p["name"])
        if line.length > 0 and not any(j in line_of for j in same):
            labels.append((p, left_to_right(line), "mapped", find_parent(line)))

    def nearest_parent(k):
        j = labels[k][3]
        while j is not None and j not in line_of:
            j = area_parent[j]
        return line_of.get(j)

    def first_zoom(k, px):
        res = 2 * math.pi * R / 256
        return next(z for z in range(30) if labels[k][1].length / (res / 2**z) >= px)

    parents = [nearest_parent(k) for k in range(len(labels))]
    first_child = {}
    for k, pk in enumerate(parents):
        if pk is not None:
            first_child[pk] = min(first_child.get(pk, 99), first_zoom(k, MIN_PX))
    zooms = {}

    # A label is shown from the zoom its parent hands over at, or later if its line is still
    # under MIN_PX, until its own line outgrows MAX_PX, but for at least one zoom and until
    # its first child is big enough, so the area never goes unlabelled. Children start no
    # earlier than the parent stops, so ancestors never share a zoom. Leaves never hand over.
    def zoom_range(k):
        if k in part_of:
            return zoom_range(part_of[k])
        if k not in zooms:
            pk = parents[k]
            minzoom = max(zoom_range(pk)[1] if pk is not None else 0, first_zoom(k, MIN_PX))
            maxzoom = (
                max(first_zoom(k, MAX_PX), minzoom + 1, first_child[k]) if k in first_child else None
            )
            zooms[k] = (minzoom, maxzoom)
        return zooms[k]

    out = []
    for k, (p, line, method, _) in enumerate(labels):
        minzoom, maxzoom = zoom_range(k)
        out.append({
            "type": "Feature",
            "geometry": mapping(to_wgs(line)),
            "properties": {
                "name": p["name"],
                "minzoom": minzoom,
                "maxzoom": maxzoom,
                "osm_id": f"{p['@type']}/{p['@id']}",
                "parent": labels[parents[k]][0]["name"] if parents[k] is not None else None,
                "method": method,
            },
        })

    with open(dst, "w") as fh:
        json.dump({"type": "FeatureCollection", "features": out}, fh, ensure_ascii=False)
    print(f"{len(out)} labels", file=sys.stderr)


if __name__ == "__main__":
    main(*sys.argv[1:])
