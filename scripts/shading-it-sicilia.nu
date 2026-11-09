#!/usr/bin/env nu

# Shaded relief for Sicilia at z16, from its 2 m lidar.
#
# Source: /run/media/martin/14TB/DEM/it/out/sicilia.tif — already warped to
#   EPSG:3857 on the XYZ z16 grid (2.3887 m), Int32 in 2 mm steps, nodata
#   -2147483648. Read-only; it belongs to the Italy source build.
#
# LICENCE CC BY 4.0, Regione Siciliana — MDT 2 m from the ATA 2012-13 flight. The credit is in write-dtm-attribution.nu under `it_sicilia`.
#
# z16 IS THE SOURCE'S OWN GRID, so there is nothing to measure: drawing it finer
#   would interpolate and drawing it coarser would throw data away. Only the
#   0.5 m tier needed a sample, because there z18 exceeded the source grid and
#   might have bought nothing — it did not.
#
# NOT italy.vrt, which is on the z17 grid and has already resolved the stack.
#   Each source is cut from its own raster and the renderer resolves overlap
#   from the masks, so every tile credits only the authority it drew from.
#
# Int32 IN 2 mm STEPS, so `unscale` is set. gdaldem ignores the band's Scale tag
#   and would hillshade heights 500x too large.
#
# STEP IS 6000 m, which is 2512 px on a 2.3887 m source. The step is in metres
#   and does not transfer between sources of different resolution.
#
# Resumable at window granularity. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-it-sicilia.nu

use lib/gdal.nu
use lib/shading.nu

const SRC       = "/run/media/martin/14TB/DEM/it/out/sicilia.tif"
const DATA_ROOT = "/mnt/osm/it_sicilia"

let DRIVE = (gdal find-drive)

shading run {
    code:      "it_sicilia"
    src:       $SRC
    unscale:   true                              # Int32, 2 mm steps
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/it/sicilia-shading.tif"
    nodata:    "-9999"
    zoom:      16                                # the source's own grid
    parallel:  6                                 # sharing the box with the contour run
    tmpdir:    "/dev/shm"
    step:      6000                              # m; 2512 px at 2.3887 m
    collar:    24                                # 10 px of context for -compute_edges
    crop:      3
    clamp:     true
    fill_md:   5
    dem_tr:    4                                 # m; what the contour run reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 1248000, y0: 4224000, id_width: 3}
}
