#!/usr/bin/env nu

# Shaded relief for the national HR-DTM at z15, from its 5 m DTM.
#
# Source: /run/media/martin/14TB/DEM/it/out/hrdtm.tif — already warped to
#   EPSG:3857 on the XYZ z15 grid (4.777 m), Int32 in 2 mm steps, nodata
#   -2147483648. Read-only; it belongs to the Italy source build.
#
# LICENCE CC BY 4.0, CNR-IRPI — HR-DTM 5 m v1.2, the fallback beneath every other Italian source. The credit is in write-dtm-attribution.nu under `it_hrdtm`.
#
# z15 IS THE SOURCE'S OWN GRID. Nothing to measure: finer interpolates, coarser
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
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-it-hrdtm.nu

use lib/gdal.nu
use lib/shading.nu

const SRC       = "/run/media/martin/14TB/DEM/it/out/hrdtm.tif"
const DATA_ROOT = "/mnt/osm/it_hrdtm"

let DRIVE = (gdal find-drive)

shading run {
    code:      "it_hrdtm"
    src:       $SRC
    unscale:   true                              # Int32, 2 mm steps
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/it/hrdtm-shading.tif"
    nodata:    "-9999"
    zoom:      15                                # the source's own grid
    parallel:  10
    tmpdir:    "/dev/shm"
    step:      12000                             # m; 2512 px at 4.777 m
    collar:    48                                # 10 px of context for -compute_edges
    crop:      3
    clamp:     true
    fill_md:   5
    dem_tr:    8                                 # m; what the contour run reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 624000, y0: 4068000, id_width: 3}
}
