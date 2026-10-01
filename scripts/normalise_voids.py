#!/usr/bin/env python3
"""Rewrite whole-tile -9999 voids to 0.00 so they survive -srcnodata 0.

Masking the sea with -srcnodata 0 takes the nodata marker away from -9999,
which would turn a void tile into terrain 9999 m down. Only a tile that is
constant can be entirely void, and a constant tile DEFLATEs to a few kilobytes,
so the candidates are found by file size and the rest of the dataset is never
read.

Usage: normalise_voids.py <dir> [size_limit_bytes] [workers]
"""
import glob
import os
import sys
from multiprocessing import Pool

import numpy as np
from osgeo import gdal

gdal.UseExceptions()


def fix(f):
    ds = gdal.Open(f)
    a = ds.GetRasterBand(1).ReadAsArray()
    ds = None
    u = np.unique(a)
    if u.size != 1 or u[0] > -9998:
        return None
    ds = gdal.Open(f, gdal.GA_Update)
    b = ds.GetRasterBand(1)
    b.WriteArray(np.zeros_like(a))
    b.SetNoDataValue(0.0)
    b.FlushCache()
    ds.FlushCache()
    ds = None
    return f


def main():
    d = sys.argv[1]
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 20000
    workers = int(sys.argv[3]) if len(sys.argv) > 3 else 8
    cand = [f for f in glob.glob(d + '/*.tif') if os.path.getsize(f) < limit]
    with Pool(workers) as p:
        done = [f for f in p.map(fix, cand, chunksize=32) if f]
    print(f'{len(cand)} constant-sized candidates, {len(done)} whole-tile voids '
          f'rewritten to 0.00')


if __name__ == '__main__':
    main()
