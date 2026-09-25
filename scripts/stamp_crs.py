#!/usr/bin/env python3
"""Stamp a CRS onto rasters that ship without one.

Some deliveries georeference a GeoTIFF but leave the projection out entirely,
documenting the CRS only in prose or in a .tfw sidecar that says nothing about
it. Hessen's DGM1 is one: every tile has a correct origin and pixel size and no
projection at all. `gdalbuildvrt` then either drops the odd ones out — silently,
in a warning swallowed by its progress bar — or, when they are uniformly blank
as here, yields a VRT with no CRS that fails the warp hours later with
"Cannot find coordinate operations".

Metadata only: no pixels are read or rewritten.

Usage: stamp_crs.py <dir> <epsg> [workers]
"""
import glob
import os
import sys
from multiprocessing import Pool

from osgeo import gdal, osr

gdal.UseExceptions()

_wkt = None


def init(epsg):
    global _wkt
    srs = osr.SpatialReference()
    srs.ImportFromEPSG(int(epsg))
    _wkt = srs.ExportToWkt()


def one(path):
    try:
        ds = gdal.Open(path)
        has = bool(ds.GetProjection())
        ds = None
        if has:
            return 0
        ds = gdal.Open(path, gdal.GA_Update)
        ds.SetProjection(_wkt)
        ds = None
        return 1
    except Exception as e:
        print(f"  FAILED {os.path.basename(path)}: {e}", file=sys.stderr)
        return -1


def main():
    d, epsg = sys.argv[1], sys.argv[2]
    workers = int(sys.argv[3]) if len(sys.argv) > 3 else 12
    files = sorted(glob.glob(os.path.join(d, "*.tif")))
    if not files:
        print(f"no rasters in {d}")
        return
    with Pool(workers, initializer=init, initargs=(epsg,)) as p:
        res = list(p.imap_unordered(one, files, chunksize=64))
    print(f"{len(files)} rasters: stamped {res.count(1)}, "
          f"already had one {res.count(0)}, failed {res.count(-1)}")
    if res.count(-1):
        sys.exit(1)


if __name__ == "__main__":
    main()
