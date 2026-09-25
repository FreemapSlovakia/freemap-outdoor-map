#!/usr/bin/env python3
"""Give every raster in a directory the same CRS.

Two deliveries have needed this, for opposite reasons:

  * Hessen ships 7389 of 22776 rasters with NO projection at all and the rest
    with EPSG:25832. `gdalbuildvrt` keeps the majority and SKIPS the rest,
    reporting it only in a warning swallowed by its progress bar.
  * Rheinland-Pfalz ships 21082 rasters as COMPOUNDCRS (ETRS89 / UTM 32N +
    DHHN2016 height) and 78 as the plain PROJCRS. Same horizontal system, so
    the pixels are interchangeable, but GDAL refuses to mix them and says so
    unhelpfully: "expected ETRS89 / UTM zone 32N, got ETRS89 / UTM zone 32N".

Mixed is worse than uniformly absent: uniformly absent fails loudly at warp
time, mixed silently drops a minority of the state.

The target is either an EPSG code or a reference raster whose projection is
copied — use the reference form to normalise onto the majority and keep
whatever extra information it carries, such as a vertical datum.

Metadata only: no pixels are read or rewritten.

Usage:
  stamp_crs.py <dir> <epsg>            e.g. stamp_crs.py /data/he 25832
  stamp_crs.py <dir> <reference.tif>   copy that file's projection
  stamp_crs.py <dir> majority          use whichever CRS most rasters have
"""
import glob
import os
import sys
from collections import Counter
from multiprocessing import Pool

from osgeo import gdal, osr

gdal.UseExceptions()

_target = None


def init(wkt):
    global _target
    _target = wkt


def read_wkt(path):
    ds = gdal.Open(path)
    w = ds.GetProjection()
    ds = None
    return w


def one(path):
    try:
        if read_wkt(path) == _target:
            return 0
        ds = gdal.Open(path, gdal.GA_Update)
        ds.SetProjection(_target)
        ds = None
        return 1
    except Exception as e:
        print(f"  FAILED {os.path.basename(path)}: {e}", file=sys.stderr)
        return -1


def main():
    d, target = sys.argv[1], sys.argv[2]
    workers = int(sys.argv[3]) if len(sys.argv) > 3 else 12
    files = sorted(glob.glob(os.path.join(d, "*.tif")))
    if not files:
        print(f"no rasters in {d}")
        return

    if target == "majority":
        # Sample rather than read all of them; the split is never subtle.
        step = max(1, len(files) // 500)
        counts = Counter(read_wkt(f) for f in files[::step])
        wkt, n = counts.most_common(1)[0]
        print(f"majority CRS from {sum(counts.values())} sampled: "
              f"{n} share it, {len(counts)} distinct")
    elif target.lower().endswith((".tif", ".tiff", ".vrt")):
        wkt = read_wkt(target)
    else:
        srs = osr.SpatialReference()
        srs.ImportFromEPSG(int(target))
        wkt = srs.ExportToWkt()

    if not wkt:
        sys.exit("target CRS is empty — refusing to stamp nothing onto everything")
    print(f"target: {wkt[:70]}...")

    with Pool(workers, initializer=init, initargs=(wkt,)) as p:
        res = list(p.imap_unordered(one, files, chunksize=64))
    print(f"{len(files)} rasters: changed {res.count(1)}, "
          f"already matched {res.count(0)}, failed {res.count(-1)}")
    if res.count(-1):
        sys.exit(1)


if __name__ == "__main__":
    main()
