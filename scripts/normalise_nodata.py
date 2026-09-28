#!/usr/bin/env python3
"""Rewrite one nodata marker to another, in place, across a directory.

A delivery that declares one nodata value and writes a second is not unusual —
Hamburg's DGM1 declares -3.4028235e+38 and writes -9999 for most of its voids,
but three of its 880 tiles use the declared value as well. Stamping the common
marker as the band's nodata then leaves the other one as valid data, and the
first thing that notices is gdal_contour refusing a range of 3.4e38.

Only tiles that actually carry the source value are rewritten.

    normalise_nodata.py <dir> <from> <to> [workers]
    normalise_nodata.py <dir> below:-1e30 -9999 12
"""
import glob
import sys
from multiprocessing import Pool

import numpy as np
from osgeo import gdal

gdal.UseExceptions()


def fix(args):
    path, src, dst = args
    ds = gdal.Open(path, gdal.GA_Update)
    band = ds.GetRasterBand(1)
    a = band.ReadAsArray()
    m = (a < float(src.split(':')[1])) if src.startswith('below:') else (a == float(src))
    n = int(m.sum())
    if n:
        a[m] = dst
        band.WriteArray(a)
        band.FlushCache()
        ds.FlushCache()
    ds = None
    return (path.rsplit('/', 1)[-1], n) if n else None


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        return 2
    d, src, dst = sys.argv[1], sys.argv[2], float(sys.argv[3])
    workers = int(sys.argv[4]) if len(sys.argv) > 4 else 8
    files = sorted(glob.glob(d + '/*.tif'))
    with Pool(workers) as p:
        out = [r for r in p.imap_unordered(fix, ((f, src, dst) for f in files), chunksize=8) if r]
    print('rewrote %d of %d rasters, %d pixels' % (len(out), len(files), sum(r[1] for r in out)))
    for name, n in sorted(out, key=lambda r: -r[1])[:8]:
        print('   %s  %d px' % (name, n))
    return 0


if __name__ == '__main__':
    sys.exit(main())
