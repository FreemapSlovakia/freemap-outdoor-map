#!/usr/bin/env python3
"""Put stray cell-centre-registered tiles back on the delivery's own grid.

Rheinland-Pfalz ships 78 of its 21160 rasters from a different pipeline: they
are 1001x1001 rather than 1000x1000, their origin sits at (-0.5, +0.5) from
the kilometre corner their filename names, and they carry the plain PROJCRS
where the rest carry COMPOUNDCRS. They are cell-centre registered and include
both shared edges, so they overlap their neighbours by half a pixel.

Left alone they are not merely 78 odd tiles: `gdalbuildvrt` takes the union, so
a half-pixel origin propagates to the whole mosaic and shifts the other 21082
tiles by up to 0.5 m. The extent comes out at x.5 with an odd row count.

Each is resampled onto the 1000x1000 kilometre tile its name declares. A half
pixel is a two-point average with bilinear, which on 1 m lidar is far less of a
lie than moving the tile half a metre and calling it aligned.

Usage: regrid_offgrid_tiles.py <dir> [resampling]
"""
import glob
import os
import shutil
import sys
import tempfile

from osgeo import gdal

gdal.UseExceptions()


def expected_corner(name):
    """Kilometre corner the filename declares: dgm1_32_<E>_<N>_1_rp_<year>."""
    p = os.path.basename(name).split("_")
    return int(p[2]) * 1000, int(p[3]) * 1000


def main():
    d = sys.argv[1]
    resample = sys.argv[2] if len(sys.argv) > 2 else "bilinear"
    files = sorted(glob.glob(os.path.join(d, "*.tif")))
    fixed = failed = 0

    for f in files:
        ds = gdal.Open(f)
        gt = ds.GetGeoTransform()
        sx, sy = ds.RasterXSize, ds.RasterYSize
        wkt = ds.GetProjection()
        nd = ds.GetRasterBand(1).GetNoDataValue()
        ds = None
        if gt[0] % 1 == 0 and gt[3] % 1 == 0 and (sx, sy) == (1000, 1000):
            continue

        x0, y0 = expected_corner(f)
        tmp = tempfile.mktemp(suffix=".tif", dir=d)
        try:
            gdal.Translate(
                tmp, f,
                projWin=[x0, y0 + 1000, x0 + 1000, y0],
                width=1000, height=1000,
                resampleAlg=resample,
                noData=nd if nd is not None else -9999,
                creationOptions=["COMPRESS=LZW", "PREDICTOR=3", "TILED=YES"],
            )
            chk = gdal.Open(tmp)
            g2 = chk.GetGeoTransform()
            ok = (chk.RasterXSize, chk.RasterYSize) == (1000, 1000) and \
                 g2[0] == x0 and g2[3] == y0 + 1000
            chk = None
            if not ok:
                raise RuntimeError(f"regrid produced {g2[0]},{g2[3]}")
            shutil.move(tmp, f)
            # gdal.Translate can drop a compound CRS; put the original back.
            out = gdal.Open(f, gdal.GA_Update)
            out.SetProjection(wkt)
            out = None
            fixed += 1
        except Exception as e:
            if os.path.exists(tmp):
                os.remove(tmp)
            print(f"  FAILED {os.path.basename(f)}: {e}", file=sys.stderr)
            failed += 1

    print(f"regridded {fixed}, failed {failed}, of {len(files)} rasters")
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
