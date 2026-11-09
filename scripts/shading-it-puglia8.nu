#!/usr/bin/env nu

# Shaded relief for Puglia at z14, from its 8 m DTM.
#
# Source: /run/media/martin/14TB/DEM/it/out/puglia8.tif — already warped to
#   EPSG:3857 on the XYZ z14 grid (9.555 m), Int32 in 2 mm steps, nodata
#   -2147483648. Read-only; it belongs to the Italy source build.
#
# LICENCE IODL 2.0, Regione Puglia — DTM 8 m from the CTR 1:5000, 2006-07. The credit is in write-dtm-attribution.nu under `it_puglia8`.
#
# z14 IS THE SOURCE'S OWN GRID. Nothing to measure: finer interpolates, coarser
#   discards. Only the 0.5 m tier needed a sample.
#
# RENDERED WHOLE, NOT CLIPPED TO WHAT SURVIVES THE STACK. Much of this dataset
#   is masked at draw time by the finer sources above it, so the stored raster
#   holds ground that is never shown. At z15 that is cheap — a 12000 m window is
#   2512 px, so the whole tier is tens of GB — and clipping would leave a hole if
#   a finer source were ever withdrawn.
#
# NOT italy.vrt, which is on the z17 grid and has already resolved the stack.
#   Each source is cut from its own raster so every tile credits only the
#   authority it drew from.
#
# Int32 IN 2 mm STEPS, so `unscale` is set.
#
# Resumable at window granularity. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-it-basilicata5.nu

use lib/gdal.nu
use lib/shading.nu

const SRC       = "/run/media/martin/14TB/DEM/it/out/puglia8.tif"
const DATA_ROOT = "/mnt/osm/it_puglia8"

let DRIVE = (gdal find-drive)

shading run {
    code:      "it_puglia8"
    src:       $SRC
    unscale:   true                              # Int32, 2 mm steps
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/it/puglia8-shading.tif"
    nodata:    "-9999"
    zoom:      14                                # the source's own grid
    parallel:  10
    tmpdir:    "/dev/shm"
    step:      24000                             # m; 2512 px at 9.555 m
    collar:    96                                # 10 px of context for -compute_edges
    crop:      3
    clamp:     true
    fill_md:   5
    dem_tr:    16                                # m; what the contour run reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 1560000, y0: 4680000, id_width: 3}
}
