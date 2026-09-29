#!/usr/bin/env python3
"""Mask pixels below a threshold in one raster, in place.

For localised spikes: a patch of a delivery that collapses toward zero inside
ground hundreds of metres up. Saarland has two such tiles, each a few hundred
pixels, and left alone they produce contour rings from 0 m upward and top the
roughness ranking with relief that is not there.

The threshold belongs to the tile, not the country: a state-wide cut deletes
real low ground elsewhere. Pick it below the tile's own p0.05 and above the
spike.

    mask_below.py <raster> <threshold>
"""
import sys

import numpy as np
from osgeo import gdal

gdal.UseExceptions()


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    path, thresh = sys.argv[1], float(sys.argv[2])
    ds = gdal.Open(path, gdal.GA_Update)
    band = ds.GetRasterBand(1)
    a = band.ReadAsArray()
    nd = band.GetNoDataValue()
    if nd is None:
        print('no nodata declared on %s — refusing to mask' % path)
        return 1
    m = (a != nd) & (a < thresh)
    n = int(m.sum())
    if n:
        a[m] = nd
        band.WriteArray(a)
        band.FlushCache()
        ds.FlushCache()
    ds = None
    print(n)
    return 0


if __name__ == '__main__':
    sys.exit(main())
