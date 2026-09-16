#!/usr/bin/env nu

# Generate shaded relief for POLAND from the GUGiK 1 m DTM.
# (The file is named shading.nu for historical reasons — it was the first of
# these scripts. Every other country is shading-<cc>.nu.)
#
# Source: /run/media/martin/4TB/PL/poland_dtm.vrt — the national 1 m mosaic
#   assembled by fetch_dtm.sh from GUGiK's WCS, EPSG:2180 (ETRF2000-PL / CS92).
#
# NODATA IS 0, AND THAT IS THE ONE THING TO REMEMBER HERE. GUGiK's WCS returns 0
#   both for out-of-coverage AND for genuine 0.00 m coastal terrain (Zulawy, the
#   Vistula delta, the whole Baltic shore). Everything downstream inherits the
#   ambiguity:
#     - gdaldem treats those real 0 m pixels as nodata and emits 0, which shows
#       as opaque specks (A=255, mask=0) scattered over flat land. gdal_fillnodata
#       with a small -md interpolates the specks away while leaving large
#       out-of-coverage regions as nodata, so they stay transparent via the mask.
#     - gdal_contour honours nodata, so the same specks notch the low contours.
#       contours-pl.nu deals with that end.
#   Do NOT "fix" this by restamping the VRT to -9999: 0 IS a legitimate elevation
#   here, and there is no sentinel in the delivery that distinguishes the two.
#
# ZOOM=16, the 1 m-source setting (same as Croatia and Norway). Poland spans
#   49-55 degN, so z16 = 2.39 3857-m/px lands at ~1.4-1.6 ground m/px against a
#   1 m source: safely oversampled. z15 would be ~2.8-3.2 ground m/px, which
#   discards real detail.
#
# Smoothing is 11/16/6/6. The filter is a PIXEL count, so 11 px = 11 m on 1 m
#   data. Italy's softer 9/15/5/5 was scaled for its 5 m pixels; do not copy it.
#
# CONVERTED FROM gdal_retile TO THE SHARED WINDOW GRID (scripts/lib/shading.nu).
#   The old pipeline wrote retiled/ and then smooth/ — two full copies of Poland
#   on disk — before hillshading. Windows are now cut from the VRT on demand,
#   smoothed in /dev/shm and deleted on the way out. It also gains resumability
#   at window granularity, .empty markers, the failed/ retry path, the
#   band-count guard and the gdalbuildvrt-drop check, none of which the retile
#   version had.
#
#   TILE IDS ARE NEW and smooth2m/ did not exist before, so this needs a FULL
#   RE-RENDER. contours-pl.nu now reads smooth2m/ and its speck-heal and
#   per-tile-crop stages are gone with the overlap they existed to undo.
#   Delete tiles/, retiled/ and smooth/ before the first run.
#
# THE GRID ORIGIN IS PINNED at (165000, 130000), below the VRT extent
#   (168000, 132000). Deriving it from the extent was the Belgium trap: adding
#   coverage moves every window id and a resumable run treats finished tiles as
#   pending. Changing these renames every tile.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING: feature-preserving-smoothing
#   reads via wbgeotiff, which ignores the TIFF Predictor tag (317) and decodes
#   PREDICTOR=2/3 float data as garbage WITHOUT erroring. The shared library
#   owns this — CO_DEM in lib/gdal.nu.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/run/media/martin/4TB/PL"
const SRC      = "/run/media/martin/4TB/PL/poland_dtm.vrt"
const EPSG     = "EPSG:2180"                     # ETRF2000-PL / CS92

gdal require-proj $EPSG "shading.nu"

shading run {
    code:      "pl"
    src:       $SRC
    data_root: $DATA_DIR
    tiles_dir: $"($DATA_DIR)/tiles"
    out_tif:   $"($DATA_DIR)/shading.tif"
    nodata:    "0"                               # AMBIGUOUS BY DELIVERY — see header
    zoom:      16                                # 1 m source — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   5                                 # px; heals the 0-specks — see header
    dem_tr:    2                                 # m; what contours-pl.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 165000, y0: 130000, id_width: 3}
}
