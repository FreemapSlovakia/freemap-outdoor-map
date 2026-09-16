#!/usr/bin/env python3
# Does the footprint bitset ever claim "no data" where there is data?
#
# The renderer skips a hillshading dataset before checking a handle out of the pool
# when no footprint cell under the tile is set (src/render/layers/hillshading_footprint.rs).
# Over-accepting only costs a wasted read; under-accepting silently drops shading,
# which is invisible until someone looks at a tile. This mirrors the Rust build
# exactly and compares it against an exact, unresampled read of a finer mask level.
#
# The reference is the finest level that can be read whole, so a clean run means "the
# footprint loses nothing the mask still holds a few levels down", not "nothing at all".
#
# Usage: ./verify-footprint.py [base] [country ...]
#        ./verify-footprint.py --hash [base] [country ...]   (to compare against the
#        Rust `against_real_data` test, which prints the same line)

import sys
import os
import glob
import time
import numpy as np
from osgeo import gdal

gdal.UseExceptions()

# Keep in sync with hillshading_footprint.rs.
GRID = 256
SOURCE_PER_CELL = 16
MAX_SOURCE_PIXELS = 32 << 20
DILATION = 2

# The reference is read finer than the footprint's own source, so it needs more room.
MAX_REFERENCE_PIXELS = 256 << 20


def levels(band):
    """Every level of a band as (level, band), the band itself last."""

    return [(i, band.GetOverview(i)) for i in range(band.GetOverviewCount())] + [(None, band)]


def source_band(ds):
    """The band the footprint is built from, and the value that means nodata.

    The same two tests read_rgba_from_gdal uses: a per-dataset mask reading 0, or the
    alpha band reading its nodata value. `_` has neither and is never skipped.
    """

    alpha = ds.GetRasterBand(4)

    if alpha.GetMaskFlags() & gdal.GMF_PER_DATASET:
        return alpha.GetMaskBand(), 0

    nodata = alpha.GetNoDataValue()

    return (alpha, int(nodata)) if nodata is not None else (None, None)


def pick_source(band):
    affordable = [
        (level, band) for level, band in levels(band)
        if band.XSize * band.YSize <= MAX_SOURCE_PIXELS
    ]

    if not affordable:
        return None

    wanted = GRID * SOURCE_PER_CELL

    resolving = [
        (level, band) for level, band in affordable
        if band.XSize >= wanted and band.YSize >= wanted
    ]

    if resolving:
        return min(resolving, key=lambda lb: lb[1].XSize * lb[1].YSize)

    return max(affordable, key=lambda lb: lb[1].XSize * lb[1].YSize)


def pick_reference(source, level):
    """The finest level that can still be read whole, and that is finer than the source.

    `None` when there is none — including when the footprint was built from the band
    itself, which has nothing below it. Then nothing can be proven and the run says so,
    rather than comparing the footprint against its own input and always passing.
    """

    if level is None:
        return None

    finer = [
        (candidate, band) for candidate, band in levels(source)
        if candidate is not None and candidate < level
        and band.XSize * band.YSize <= MAX_REFERENCE_PIXELS
    ]

    return max(finer, key=lambda lb: lb[1].XSize * lb[1].YSize) if finer else None


def footprint_of(band, nodata, gw, gh):
    """The Rust build: OR every source pixel into its cell, then dilate by DILATION."""

    data = band.ReadAsArray() != nodata
    h, w = data.shape

    rows = np.arange(h) * gh // h
    columns = np.arange(w) * gw // w

    folded = np.zeros((gh, gw), dtype=bool)

    for start in range(0, h, 512):
        slab = data[start:start + 512]
        ys, xs = np.nonzero(slab)

        folded[rows[start + ys], columns[xs]] = True

    dilated = folded.copy()

    for _ in range(DILATION):
        grown = dilated.copy()

        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                grown |= shift(dilated, dy, dx)

        dilated = grown

    return folded, dilated


def shift(cells, dy, dx):
    """Like np.roll, but the grid edge drops what falls off it instead of wrapping."""

    out = np.zeros_like(cells)
    h, w = cells.shape

    out[max(0, dy):h - max(0, -dy), max(0, dx):w - max(0, -dx)] = \
        cells[max(0, -dy):h - max(0, dy), max(0, -dx):w - max(0, dx)]

    return out


def reference_of(band, nodata, gw, gh):
    """Cell holds data if any pixel in the range it covers does, rounded outward."""

    data = band.ReadAsArray() != nodata
    h, w = data.shape

    xs = [(i * w // gw, -(-((i + 1) * w) // gw)) for i in range(gw)]
    ys = [(j * h // gh, -(-((j + 1) * h) // gh)) for j in range(gh)]

    rows = np.array([data[y0:y1].any(axis=0) for y0, y1 in ys])

    return np.array([[rows[j, x0:x1].any() for x0, x1 in xs] for j in range(gh)])


def check(path, name):
    ds = gdal.Open(path)

    if ds.RasterCount != 4:
        print(f"{name}: {ds.RasterCount} bands, the renderer expects 4")
        return 0

    source, nodata = source_band(ds)

    if source is None:
        print(f"{name}: nothing marks nodata, never skipped")
        return 0

    picked = pick_source(source)

    if picked is None:
        print(f"{name}: no level under {MAX_SOURCE_PIXELS} pixels -> no footprint")
        return 0

    level, band = picked
    gw, gh = min(GRID, band.XSize), min(GRID, band.YSize)

    started = time.time()
    folded, footprint = footprint_of(band, nodata, gw, gh)
    took = time.time() - started

    picked_reference = pick_reference(source, level)

    if picked_reference is None:
        print(f"{name}: no level finer than the source can be read, nothing to compare")
        return 0

    ref_level, ref_band = picked_reference
    reference = reference_of(ref_band, nodata, gw, gh)

    missed = int((reference & ~footprint).sum())
    data = int(reference.sum())
    cells = gw * gh

    print(
        f"{name}: {'mask' if nodata == 0 else 'alpha'} {source.XSize}x{source.YSize}, "
        f"{source.GetOverviewCount()} overviews, "
        f"source level {level} {band.XSize}x{band.YSize} ({band.XSize * band.YSize / 1e6:.1f} Mpx, "
        f"{took * 1000:.0f} ms), grid {gw}x{gh}, "
        f"reference level {ref_level} {ref_band.XSize}x{ref_band.YSize}, "
        f"data {100.0 * data / cells:.1f}% of bbox, "
        f"accept {100.0 * int(folded.sum()) / cells:.1f}% -> {100.0 * int(footprint.sum()) / cells:.1f}% dilated, "
        f"missed {missed}"
        + ("  <-- LOSES DATA" if missed else "")
    )

    return missed


def show_hash(path, name):
    """The footprint in the form the Rust `against_real_data` test prints it."""

    ds = gdal.Open(path)
    source, nodata = source_band(ds)

    if source is None:
        print(f"{name}: nothing marks nodata, never skipped")
        return 0

    picked = pick_source(source)

    if picked is None:
        print(f"{name}: no level under {MAX_SOURCE_PIXELS} pixels -> no footprint")
        return 0

    level, band = picked
    gw, gh = min(GRID, band.XSize), min(GRID, band.YSize)
    _, dilated = footprint_of(band, nodata, gw, gh)

    words_per_row = -(-gw // 64)
    packed = np.zeros(words_per_row * gh, dtype=np.uint64)

    for row in range(gh):
        for x in np.nonzero(dilated[row])[0]:
            packed[row * words_per_row + x // 64] |= np.uint64(1) << np.uint64(x % 64)

    digest = 0xCBF29CE484222325

    for byte in packed.tobytes():
        digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF

    print(
        f"{name}: grid {gw}x{gh} words {words_per_row} "
        f"set {int(dilated.sum())} hash {digest:016x}"
    )

    return 0


def main():
    argv = sys.argv[1:]
    hashing = argv and argv[0] == "--hash"

    if hashing:
        argv = argv[1:]

    base = argv[0] if argv else "/fm/data2/hillshading"
    names = argv[1:]

    if not names:
        names = sorted(
            os.path.basename(os.path.dirname(p))
            for p in glob.glob(os.path.join(base, "*", "final.tif"))
        )

    total = 0

    for name in names:
        path = os.path.join(base, name, "final.tif")

        try:
            total += show_hash(path, name) if hashing else check(path, name)
        except RuntimeError as err:
            print(f"{name}: {err}")

    if not hashing:
        print(f"\n{'FAIL' if total else 'ok'}: {total} cells claim empty but hold data")

    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
