#!/usr/bin/env python3
"""Drop the one-pixel ramp that hugs a nodata boundary.

Baden-Württemberg's DGM1 writes out-of-coverage as 0.00 rather than flagging
it, and the cells immediately inside the coverage edge hold a partial value
between the real ground and that zero. Masking the zeros alone therefore leaves
a rim that falls hundreds of metres in one pixel — in the Allgäu, from ~780 m
down to 48 m — and feature-preserving smoothing treats a step that sharp as a
feature and keeps it, so the repair has to happen before smoothing.

Measured on dgm1_32_581_5276: 742 such pixels, every one of them adjacent to
the nodata region and none isolated, so eroding the valid mask by one pixel
removes the rim and nothing else.

Usage: erode_nodata_fringe.py <src> <dst> [iterations]
"""
import sys
import numpy as np
from osgeo import gdal

gdal.UseExceptions()


def neighbours_invalid(bad):
    """True where any 4-neighbour is invalid."""
    out = np.zeros_like(bad)
    out[1:, :] |= bad[:-1, :]
    out[:-1, :] |= bad[1:, :]
    out[:, 1:] |= bad[:, :-1]
    out[:, :-1] |= bad[:, 1:]
    return out


def main():
    src, dst = sys.argv[1], sys.argv[2]
    iters = int(sys.argv[3]) if len(sys.argv) > 3 else 1

    ds = gdal.Open(src)
    band = ds.GetRasterBand(1)
    nd = band.GetNoDataValue()
    if nd is None:
        nd = -9999.0
    a = band.ReadAsArray().astype("float32")

    bad = (a == nd) | ~np.isfinite(a)
    if bad.any():
        for _ in range(iters):
            bad |= neighbours_invalid(bad)
        a[bad] = nd

    drv = gdal.GetDriverByName("GTiff")
    # PREDICTOR=1: feature-preserving-smoothing reads this through wbgeotiff,
    # which ignores the TIFF predictor tag and decodes 2/3 as +/-Inf.
    out = drv.Create(dst, ds.RasterXSize, ds.RasterYSize, 1, gdal.GDT_Float32,
                     ["COMPRESS=DEFLATE", "PREDICTOR=1", "TILED=YES"])
    out.SetGeoTransform(ds.GetGeoTransform())
    out.SetProjection(ds.GetProjection())
    ob = out.GetRasterBand(1)
    ob.SetNoDataValue(nd)
    ob.WriteArray(a)
    ob.FlushCache()
    out = None
    ds = None


if __name__ == "__main__":
    main()
