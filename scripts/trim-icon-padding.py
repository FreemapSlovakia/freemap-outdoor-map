#!/usr/bin/env python3
"""Trim the hand-added halo padding off icon canvases in images/.

Icons used to be drawn with 1.5 px of empty canvas on every side. The white glow the
renderer strokes under them is 3 px wide, so it reaches 1.5 px outside the outline, and
librsvg clips to the document viewport — without that margin the glow was sliced off at
the edge. `svg_repo::pad_viewport` now grows the viewport by that much in code, so the
margin no longer belongs in the file, and this removes it.

WHY TRIMMING IS A NO-OP ON THE MAP. `pois::render_icons` places an icon by its *ink*
extents, not by its canvas: it centres `ink_w`/`ink_h` on the point and paints the
surface at `corner - ink_origin`. With the glow intact the ink is the outline offset by
exactly 1.5 px in every direction, so `path_origin - ink_origin` is 1.5 px whatever the
canvas looks like, and every edge lands on the same pixel it did before. The canvas only
has to *contain* the drawing; its size is invisible downstream.

WHAT IS NOT SAFE, AND WHY EACH CASE IS LEFT ALONE.

  * Pattern tiles and line decoration (landcover fills, `waterway-arrow`, `no_foot`, …)
    are fetched with `use_extents: true` and stamped repeatedly. There the canvas *is*
    the tile — trimming it would change the spacing of the pattern.
  * The spring layers are a composite: `pois::spring_variant` merges `spring` or
    `mineral-spring` with `refitted_spring`/`drinkable_spring`/`intermittent` into one
    document, so all of them are drawn in the base file's coordinate system. Trimming
    one moves it relative to the others; they can only be trimmed together, by a common
    offset.
  * An icon that paints its own stroke has a canvas fitted to the stroke rather than to
    the fill, and Inkscape's fit-to-drawing uses the visual bounding box, so the two
    disagree about what "the drawing" is.
  * An icon whose drawing already overflows its canvas is being clipped today. Trimming
    would reveal the clipped ink — a real change, and someone should look at why.

Inkscape does the geometry (`fit-canvas-to-selection`, which rewrites the coordinates);
this script then strips what the export adds — the XML prologue, dead namespaces, its
ids nothing resolves against, an empty `<defs>` — and keeps the rest. It keeps a
`<defs>` that was already there on purpose: `SvgRepo` counts top-level elements to
decide whether to stroke the glow onto the element itself or stamp a `<use>` copy behind
it, so dropping one would silently switch an icon to the other mechanism.

Every rewrite is then verified: the drawing keeps its size and lands at the origin, the
canvas matches it, the top-level elements and every original id survive, and the icon
rasterises identically at 4x. Anything that fails is reverted and reported instead.

    python3 scripts/trim-icon-padding.py                    # dry run, writes the report
    python3 scripts/trim-icon-padding.py --apply            # rewrite images/*.svg
    python3 scripts/trim-icon-padding.py --apply bench mine # just these

Needs `inkscape` and Pillow.
"""

import argparse
import collections
import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

from PIL import Image, ImageChops

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMAGES = os.path.join(REPO, "images")
SVG = "{http://www.w3.org/2000/svg}"
REPORT = os.path.join(REPO, "doc", "icon-canvas-review.md")

# Tolerance for every geometric comparison, in px. Anything under this is Inkscape's
# 8-significant-digit output, not a change to the drawing.
TOL = 1e-3

# Layers of the composite spring icon - see the module docstring.
SPRING_LAYERS = {"spring", "mineral-spring", "refitted_spring", "drinkable_spring", "intermittent"}

# Ids the renderer itself selects on: spring_variant() builds stylesheets that colour
# `#spring` and `#drinkable`. Every other id is only worth keeping if the file points at
# it - a `<use href="#x">` or a `fill="url(#x)"`.
PROTECTED_IDS = {"spring", "drinkable"}

ID_REF = re.compile(r'url\(#([^)]+)\)|(?:xlink:)?href="#([^"]+)"')

# Structural elements a trim could plausibly restructure or renumber.
EXOTIC = ("use", "image", "text", "style", "linearGradient", "radialGradient",
          "filter", "clipPath", "mask")


# --------------------------------------------------------------------- reachability

def icon_roles():
    """(icons drawn with a halo, icons drawn without one, icons used for anything else).

    Read out of the source rather than listed here, so an icon added to `POI_ENTRIES`
    or to the landcover table is classified without touching this script.
    """
    srcs = {p: open(p).read() for p in glob.glob(REPO + "/src/**/*.rs", recursive=True)}

    body = srcs[REPO + "/src/render/layers/pois.rs"]
    body = body[body.index("let entries = vec!["):body.index("\n    ];")]
    body = "\n".join(l for l in body.split("\n") if not l.strip().startswith("//"))

    entry = re.compile(r'\(\s*(?:\d+|NN),\s*(?:\d+|NN),\s*[YN],\s*[YN],\s*\w+,\s*"([^"]+)",')
    matches = list(entry.finditer(body))

    halo, plain = set(), set()

    for i, m in enumerate(matches):
        rest = body[m.end():matches[i + 1].start() if i + 1 < len(matches) else len(body)]
        icon = re.search(r'icon:\s*Some\("([^"]+)"\)', rest)
        (plain if "halo: false" in rest else halo).add(icon.group(1) if icon else m.group(1))

    # spring_variant() assembles these by hand, and `ruins` substitutes for any icon
    halo |= SPRING_LAYERS | {"ruins"}

    other = set()

    for src in srcs.values():
        other |= set(re.findall(r'svg_repo\.get\("([^"]+)"\)', src))
        other |= set(re.findall(r'Paint::Pattern\("([^"]+)"\)', src))
        other |= set(re.findall(r'names:\s*vec!\["([^"]+)"', src))

    return halo, plain, other - halo


# --------------------------------------------------------------------- svg probing

def bbox(path):
    """Drawing bounds in px, or None when inkscape reports nothing drawable."""
    out = subprocess.run(["inkscape", "--query-all", path],
                         capture_output=True, text=True).stdout

    for line in out.splitlines():
        parts = line.split(",")

        if len(parts) == 5:
            try:
                return [float(v) for v in parts[1:]]
            except ValueError:
                pass

    return None


def probe(path):
    """Canvas size, structure and paint facts the classifier and the checks need."""
    root = ET.parse(path).getroot()

    def num(attr):
        value = root.get(attr)

        return (float(value.replace("px", ""))
                if value and re.fullmatch(r"[\d.]+(px)?", value) else None)

    text = open(path).read()
    tags = [child.tag.replace(SVG, "") for child in root]

    return {
        "width": num("width"),
        "height": num("height"),
        "tags": tags,
        "ids": {e.get("id") for e in root.iter() if e.get("id")},
        # a stroke of the icon's own making, which the canvas is then fitted to
        "stroked": bool(re.search(r'stroke\s*[:=]\s*"?\s*(?!none)[#a-zA-Z0-9]', text)),
        "exotic": sorted({t for t in (e.tag.replace(SVG, "") for e in root.iter())
                          if t in EXOTIC}),
        "full_defs": any(c.tag == SVG + "defs" and len(c) for c in root),
        "transform": root.get("transform"),
    }


# --------------------------------------------------------------------- pixel grid

# How far off a whole pixel a coordinate may sit and still count as on the grid. Well
# under an eighth of a pixel: below this nothing is visible, and it forgives the
# rounding warts editors leave behind (8.0032534 for an 8).
GRID_TOL = 0.02

_NUM = re.compile(r"[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?")
_CMD = re.compile(r"[MmLlHhVvCcSsQqTtAaZz]")

# arguments per command, and where in them the segment's end point sits
_ARGS = {"m": 2, "l": 2, "h": 1, "v": 1, "c": 6, "s": 4, "q": 4, "t": 2, "a": 7, "z": 0}


def path_segments(d):
    """Every segment in a `d` attribute as (start, end, is_straight). None if unreadable.

    Curves are marked but kept: only straight, axis-aligned edges are crisp or soft in
    a way a reader notices, since a curve is antialiased wherever it falls.
    """
    tokens = [[float(n) for n in _NUM.findall(part)] for part in _CMD.split(d)]
    letters = _CMD.findall(d)

    if len(tokens) != len(letters) + 1 or tokens[0]:
        return None  # leading garbage, or a `d` that does not start with a command

    x = y = sx = sy = 0.0
    segments = []

    for letter, args in zip(letters, tokens[1:]):
        low = letter.lower()
        rel = letter.islower()
        step = _ARGS[low]

        if low == "z":
            segments.append(((x, y), (sx, sy), True))
            x, y = sx, sy
            continue

        if not args or len(args) % step:
            return None  # implicit repeats do not divide evenly (unseparated arc flags?)

        for i in range(0, len(args), step):
            chunk = args[i:i + step]
            px, py = x, y

            if low == "h":
                x = x + chunk[0] if rel else chunk[0]
            elif low == "v":
                y = y + chunk[0] if rel else chunk[0]
            else:
                ex, ey = chunk[-2], chunk[-1]
                x, y = (px + ex, py + ey) if rel else (ex, ey)

            if low == "m":
                if i == 0:
                    sx, sy = x, y
                    low = "l"  # a moveto's repeats are linetos
                continue

            segments.append(((px, py), (x, y), low in ("l", "h", "v")))

    return segments


def geometry_segments(root):
    """Every segment in the document, or None if it cannot be read exactly."""
    segments = []

    for el in root.iter():
        tag = el.tag.replace(SVG, "")

        if el.get("transform"):
            return None  # would have to be applied before comparing against the bbox

        if tag == "path":
            found = path_segments(el.get("d", ""))

            if found is None:
                return None

            segments += found
        elif tag == "rect":
            rx, ry = float(el.get("x", 0)), float(el.get("y", 0))
            w, h = float(el.get("width", 0)), float(el.get("height", 0))
            corners = [(rx, ry), (rx + w, ry), (rx + w, ry + h), (rx, ry + h)]
            segments += [(corners[i], corners[(i + 1) % 4], True) for i in range(4)]
        elif tag in ("circle", "ellipse"):
            pass  # all curve, nothing axis-aligned to judge
        elif tag in ("polygon", "polyline", "line", "use", "image"):
            return None

    return segments


def off_grid(path, drawing):
    """(flat edges off the pixel grid, flat edges, worst offset in px), or None.

    `render_icons` snaps the ink box to the pixel grid, and the ink is the drawing
    offset by a flat 1.5 px, so the drawing's bounding box lands on whole pixels
    whatever its size. A straight horizontal or vertical edge is then crisp exactly
    when its constant coordinate is a whole number of pixels from that box - which is
    a property of how the icon was drawn, and no canvas change can fix or break it.
    """
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError:
        return None

    segments = geometry_segments(root)

    if not segments or drawing is None:
        return None

    bx, by = drawing[0], drawing[1]
    flat = off = 0
    worst = 0.0
    fracs = ([], [])  # where the vertical / horizontal edges fall between pixels

    for (x0, y0), (x1, y1), straight in segments:
        if not straight:
            continue

        if abs(y0 - y1) < 1e-9 < abs(x0 - x1):
            value, axis = y0 - by, 1             # horizontal edge
        elif abs(x0 - x1) < 1e-9 < abs(y0 - y1):
            value, axis = x0 - bx, 0             # vertical edge
        else:
            continue

        flat += 1
        delta = abs(value - round(value))
        worst = max(worst, delta)
        fracs[axis].append(round(value % 1.0, 2) % 1.0)

        if delta > GRID_TOL:
            off += 1

    if not flat:
        return None

    # Whether the edges on each axis at least agree with one another. They are measured
    # against the drawing's own bounding box, so a translate changes nothing - it moves
    # the reference too. One shared fraction means the icon is internally consistent and
    # only its outermost point, usually a curve, sits off from the rest; differing
    # fractions mean the edges disagree among themselves.
    agreement = tuple(0.0 if not f else
                      (next(iter(set(f))) if len(set(f)) == 1 else None)
                      for f in fracs)

    return off, flat, worst, agreement


# --------------------------------------------------------------------- the rewrite

def inkscape_trim(src, dst):
    subprocess.run(
        ["inkscape",
         "--actions=select-all;fit-canvas-to-selection;"
         "export-filename:%s;export-plain-svg;export-do" % dst,
         src],
        capture_output=True, text=True)

    return os.path.exists(dst) and os.path.getsize(dst) > 0


def _fmt(value):
    """4 dp is ~1e-4 px on a canvas the renderer then pads by 1.5 - invisible, and it
    turns Inkscape's 11.999999 back into the 12 the icon was drawn at."""
    out = ("%.4f" % value).rstrip("0").rstrip(".")

    return "0" if out in ("", "-0") else out


def tidy_root_numbers(text):
    end = text.index(">")
    head, tail = text[:end], text[end:]

    head = re.sub(r'((?:width|height)=")([-\d.eE+]+)(")',
                  lambda m: m.group(1) + _fmt(float(m.group(2))) + m.group(3), head)

    head = re.sub(r'(viewBox=")([^"]+)(")',
                  lambda m: m.group(1) + " ".join(
                      _fmt(float(v)) for v in re.split(r"[ ,]+", m.group(2).strip())) + m.group(3),
                  head)

    return head + tail


def live_ids(text):
    """Ids that something actually resolves against - the rest are dead weight."""
    return PROTECTED_IDS | {m.group(1) or m.group(2) for m in ID_REF.finditer(text)}


def postprocess(text, orig_tags):
    """Strip what the export added. Anything the original had is left alone."""
    text = re.sub(r'<\?xml[^>]*\?>\s*', '', text)
    text = re.sub(r'\s+xmlns:(sodipodi|inkscape|svg)="[^"]*"', '', text)
    text = re.sub(r'\s+(sodipodi|inkscape):[\w-]+="[^"]*"', '', text)
    text = re.sub(r'\s*<(sodipodi|inkscape):[^>]*/>', '', text)
    text = re.sub(r'\s+version="[^"]*"', '', text)  # SVG 2 dropped it

    live = live_ids(text)
    text = re.sub(r'\s+id="([^"]*)"',
                  lambda m: m.group(0) if m.group(1) in live else "", text)

    # An empty <defs> the export added is litter. One that was already there stays: it
    # counts towards SvgRepo's top-level element count, which picks the halo mechanism.
    if "defs" not in orig_tags:
        text = re.sub(r'\s*<defs\s*/>', '', text)
        text = re.sub(r'\s*<defs\s*>\s*</defs>', '', text)

    text = tidy_root_numbers(text)
    text = re.sub(r'\n\s*\n', '\n', text)

    return text.strip() + "\n"


# --------------------------------------------------------------------- verification

def render_png(path, out, dpi=384):
    subprocess.run(["inkscape", "--export-type=png", "--export-area-drawing",
                    "--export-dpi=%d" % dpi, "--export-filename=" + out, path],
                   capture_output=True, text=True)

    return os.path.exists(out)


def raster_diff(before, after, tmp):
    """(worst channel delta, differing pixels, total pixels) at 4x.

    Both are exported over their own drawing area, which the trim is supposed to leave
    identical - so a correct trim gives two identical rasters.
    """
    a, b = os.path.join(tmp, "a.png"), os.path.join(tmp, "b.png")

    if not (render_png(before, a) and render_png(after, b)):
        return None

    ia = Image.open(a).convert("RGBA")
    ib = Image.open(b).convert("RGBA")

    if ia.size != ib.size:
        return ("size", ia.size, ib.size)

    diff = ImageChops.difference(ia, ib)
    worst = differing = 0

    for pixel in diff.getdata():
        delta = max(pixel)

        if delta:
            differing += 1
            worst = max(worst, delta)

    return (worst, differing, ia.size[0] * ia.size[1])


def check(before, after, before_probe, drawing, tmp, raster_tolerance):
    """Everything that must still hold after the rewrite. Empty list means good."""
    bx, by, bw, bh = drawing
    problems = []

    try:
        now = probe(after)
    except ET.ParseError as err:
        return ["trimmed file does not parse: %s" % err], ""

    box = bbox(after) or [9e9, 9e9, 0, 0]

    if now["width"] is None or abs(now["width"] - bw) > TOL or abs(now["height"] - bh) > TOL:
        problems.append("canvas %s×%s does not match the drawing's %g×%g"
                        % (now["width"], now["height"], bw, bh))

    if max(abs(box[0]), abs(box[1]), abs(box[2] - bw), abs(box[3] - bh)) > TOL:
        problems.append("drawing moved or resized: %s" % [round(v, 4) for v in box])

    if now["tags"] != before_probe["tags"]:
        problems.append("top-level elements %s became %s" % (before_probe["tags"], now["tags"]))

    was_live = before_probe["ids"] & live_ids(open(before).read())

    if not was_live <= now["ids"]:
        problems.append("ids lost: %s" % sorted(was_live - now["ids"]))

    note = ""
    diff = raster_diff(before, after, tmp)

    if diff is None:
        problems.append("png export failed")
    elif diff[0] == "size":
        problems.append("raster is %s against %s" % (diff[1], diff[2]))
    elif diff[0] > raster_tolerance:
        problems.append("rasterises differently: worst channel delta %d/255 over %d of %d px, "
                        "where the export rounded a curve. Judge it by eye and accept it with "
                        "`--raster-tolerance %d %s` if it is not visible"
                        % (diff[0], diff[1], diff[2], diff[0],
                           os.path.basename(before)[:-4]))
    elif diff[1]:
        note = "%d of %d px differ by <=%d/255" % (diff[1], diff[2], diff[0])

    return problems, note


# --------------------------------------------------------------------- driver

def classify(name, info, drawing, roles):
    """(action, reason) for an icon the trim has not been attempted on yet."""
    halo, plain, other = roles

    if name in SPRING_LAYERS:
        return ("review", "a layer of the composite spring icon: `spring_variant` merges these "
                          "files into one document, so they share a coordinate system and can "
                          "only be trimmed together, by a common offset")

    if name in other:
        return ("skip", "used as a pattern tile or line decoration, where the canvas is the tile")

    if name in plain:
        return ("skip", "drawn with `halo: false`, so the renderer adds no padding")

    if name not in halo:
        return ("skip", "not reachable from any layer")

    if info["width"] is None or info["height"] is None:
        return ("review", "root has no plain numeric `width`/`height`")

    if drawing is None:
        return ("review", "inkscape reports no drawable content, so it cannot be measured")

    if info["exotic"]:
        return ("review", "contains `<%s>`, which the trim could restructure"
                          % ">`, `<".join(info["exotic"]))

    if info["full_defs"] or info["transform"]:
        return ("review", "has a non-empty `<defs>` or a `transform` on the root")

    if info["stroked"]:
        return ("review", "paints its own stroke, so its canvas fits the stroke and not the fill")

    bx, by, bw, bh = drawing
    pad = min(bx, by, info["width"] - (bx + bw), info["height"] - (by + bh))

    if pad < -TOL:
        return ("review", "drawing overflows its canvas by %.3f px, so it is being clipped "
                          "today and trimming would reveal the clipped ink" % -pad)

    if pad <= 0.05:
        return ("skip", "already tight (%.3f px)" % pad)

    return ("trim", "%.3f px" % pad)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("names", nargs="*", help="icon names to consider (default: all)")
    parser.add_argument("--apply", action="store_true", help="rewrite images/*.svg in place")
    parser.add_argument("--report", default=REPORT, help="where to write the markdown report")
    parser.add_argument("--raster-tolerance", type=int, default=2, metavar="N",
                        help="largest per-channel difference, out of 255, to accept between the "
                             "icon before and after (default 2: antialiasing noise only)")
    args = parser.parse_args()

    roles = icon_roles()
    rows = []
    grids = {}
    tmp = tempfile.mkdtemp(prefix="trim-icons-")

    for path in sorted(glob.glob(IMAGES + "/*.svg")):
        name = os.path.basename(path)[:-4]

        if args.names and name not in args.names:
            continue

        info = probe(path)
        drawing = bbox(path)
        action, reason = classify(name, info, drawing, roles)

        # only the icons render_icons places; pattern tiles are stamped by other code
        if name in roles[0] or name in roles[1]:
            grids[name] = off_grid(path, drawing)

        detail = ("canvas %g×%g, drawing %g,%g %g×%g"
                  % (info["width"] or 0, info["height"] or 0, *drawing)
                  if drawing and info["width"] else "canvas %s×%s" % (info["width"], info["height"]))

        if action != "trim":
            rows.append((name, action, reason, detail, ""))
            continue

        out = os.path.join(tmp, name + ".svg")

        if not inkscape_trim(path, out):
            rows.append((name, "review", "inkscape produced no output", detail, ""))
            continue

        raw = open(out).read()  # read before reopening to write: "w" truncates
        open(out, "w").write(postprocess(raw, info["tags"]))

        problems, note = check(path, out, info, drawing, tmp, args.raster_tolerance)

        if problems:
            rows.append((name, "review", "trim verification failed: " + "; ".join(problems),
                         detail, ""))
            continue

        if args.apply:
            shutil.copyfile(out, path)

        rows.append((name, "trimmed", "%s of padding removed" % reason, detail, note))

    # a filtered run only sees part of the picture, so it must not overwrite the report
    if args.names:
        print("(subset run: %s not rewritten)" % args.report)
    else:
        write_report(rows, args.report, args.apply, grids)

    for action in ("trimmed", "review", "skip"):
        chosen = [r for r in rows if r[1] == action]
        print("%-8s %d" % (action, len(chosen)))

        if action == "review":
            for name, _, reason, _, _ in chosen:
                print("   %-30s %s" % (name, reason))

    if args.names:
        print("\n%s; run without names to refresh %s"
              % ("applied" if args.apply else "dry run", args.report))
    else:
        print("\n%s %s" % ("wrote" if args.apply else "dry run, would trim; report at", args.report))


def write_report(rows, path, applied, grids):
    out = []
    add = out.append

    add("# Icon canvas review\n")
    add("`svg_repo::pad_viewport` grows every haloed icon's viewport by 1.5 px on each side")
    add("before librsvg sees it, so an icon no longer carries that padding in its own file.")
    add("`scripts/trim-icon-padding.py` removed it from the ones where that is provably a")
    add("no-op. This file is the other half: what it would not touch.\n")
    add("The first table is the one to read — those need a decision a script should not make.")
    add("Everything below it was skipped for a reason that needs no action.\n")
    add("Regenerate with `python3 scripts/trim-icon-padding.py` (add `--apply` to rewrite).\n")

    review = sorted([r for r in rows if r[1] == "review"])

    add("## Needs a look (%d)\n" % len(review))

    if review:
        add("| icon | why it was skipped | geometry |")
        add("|---|---|---|")

        for name, _, reason, detail, _ in review:
            add("| `%s.svg` | %s | %s |" % (name, reason, detail))

        add("")

    ragged = sorted(((n, g) for n, g in grids.items() if g and g[0]),
                    key=lambda item: (-item[1][2], -item[1][0] / item[1][1], item[0]))

    add("## Authored off the pixel grid (%d)\n" % len(ragged))
    add("Nothing to fix in the canvas, and nothing this script can do — listed because it")
    add("is the one thing that does still blur an icon.\n")
    add("`render_icons` snaps the ink box to the pixel grid, and the ink is the drawing")
    add("offset by a flat 1.5 px, so an icon's bounding box always lands on whole pixels —")
    add("whatever its size, odd, even or fractional. A straight horizontal or vertical edge")
    add("inside it is crisp exactly when it sits a whole number of pixels from that box. The")
    add("icons below have edges that do not, so those edges render soft at every zoom.")
    add("Curves are not counted: they are antialiased wherever they fall.\n")
    add("Moving the drawing does not help — the bounding box is measured from the drawing,")
    add("so a translate takes the reference with it. What matters is where each edge sits")
    add("*relative to the drawing's own extremes*. Where the fractions agree on an axis the")
    add("icon is internally consistent, and it is its outermost point — usually a curve —")
    add("that sits the odd half pixel away; moving the flat edges together by that fraction")
    add("(or adjusting the extreme) squares the lot up. Where they are mixed, the edges")
    add("disagree among themselves and only redrawing settles it.\n")

    if ragged:
        add("| icon | flat edges off the grid | worst offset | edges agree with each other? |")
        add("|---|---|---|---|")

        for name, (off, flat, worst, agreement) in ragged:
            verdict = ", ".join(
                "%s %s" % (axis, "mixed" if frac is None else "%g px" % frac)
                for axis, frac in zip(("vertical", "horizontal"), agreement))

            add("| `%s.svg` | %d of %d | %.3f px | %s |" % (name, off, flat, worst, verdict))

        add("")

    clean = sorted(n for n, g in grids.items() if g and not g[0])
    curved = sorted(n for n, g in grids.items() if g is None)

    add("%d icons are fully on the grid%s.\n"
        % (len(clean), "" if not curved else
           ", and %d have no straight axis-aligned edge to judge (`%s`)"
           % (len(curved), "`, `".join(curved))))

    groups = collections.OrderedDict()

    for name, action, reason, _, _ in rows:
        if action == "skip":
            groups.setdefault(reason.split(" (")[0], []).append(name)

    add("## Skipped, nothing to decide (%d)\n" % sum(len(v) for v in groups.values()))

    for reason, names in groups.items():
        add("**%s** — %d\n" % (reason[0].upper() + reason[1:], len(names)))
        add("> " + ", ".join("`%s`" % n for n in sorted(names)) + "\n")

    trimmed = sorted([r for r in rows if r[1] == "trimmed"])

    if not trimmed:      # a re-run once everything is done has nothing to list
        os.makedirs(os.path.dirname(path), exist_ok=True)
        open(path, "w").write("\n".join(out))
        return

    add("## Trimmed (%d)\n" % len(trimmed))
    add("Recorded for completeness; each one verified clean%s.\n"
        % ("" if applied else " in a dry run and was not written"))

    noted = [r for r in trimmed if r[4]]

    if noted:
        add("These rasterised with a trace of antialiasing noise where Inkscape rewrote the")
        add("path data — far below anything visible, but worth knowing:\n")

        for name, _, _, _, note in noted:
            add("- `%s.svg` — %s" % (name, note))

        add("")

    add("> " + ", ".join("`%s`" % r[0] for r in trimmed) + "\n")

    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, "w").write("\n".join(out))


if __name__ == "__main__":
    sys.exit(main())
