#!/usr/bin/env nu

# Generate contour lines for all of Luxembourg from the 2 m smoothed DEM tiles
# that shading-lu.nu emitted along the way.
# Luxembourg port of contours-en.nu (England 2 m) / contours-no.nu (Norway 2 m).
#
# Pipeline: /mnt/osm/lu/smooth2m/*.tif   (1250x1250 px windows, 2 m, EPSG:2169)
#             -> one national VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:2169)
#
# As in every country now, there is NO per-tile cropping and
# NO 1 m -> 2 m downsampling to do here: shading-lu.nu's dem2m step already wrote
# each window cropped to its exact 2.5 km extent (-projwin at the window bounds,
# collar excluded) and already at 2 m, produced with nodata-aware `average` while
# the smoothed DEM was still in RAM. So these tiles tile the plane seamlessly with
# no overlap and no gaps, and gdalbuildvrt is enough.
#
# THE CONTOURS MUST COME FROM THE SMOOTHED DEM. Contouring raw 1 m LiDAR produces
# unusable spaghetti — every furrow and forest-floor speckle becomes a closed
# loop. That is the entire reason the shading run bothers to emit smooth2m/
# rather than letting this script downsample the national raster itself. If
# /mnt/osm/lu/smooth2m is empty, run shading-lu.nu first; do NOT point this script
# at luxembourg_dem_1m.tif as a shortcut.
#
# The consolidation pass is NOT skipped. gdal_contour over a many-hundred-tile VRT
# is pathologically slow — scattered reads, tiles reopened per scanline — so the
# tiles still have to be merged into one contiguous raster first. Luxembourg is
# small enough that this is minutes, not the 8.5 h it cost Croatia.
#
# DATUM: NO GRID SHIFT EXISTS, AND NONE IS NEEDED. projinfo offers exactly two
#   EPSG:2169 -> EPSG:4326 operations, LUREF to WGS 84 (4) (Molodensky-Badekas)
#   and (3) (7-parameter Helmert), both stated at 1 m, NEITHER referencing an
#   NTv2 grid. There is no Luxembourg OSTN15 to install and therefore none of
#   England's mixed-datum hazard: nothing can silently change under a re-run.
#   This GPKG is native EPSG:2169 and unaffected regardless.
#
# NODATA. The 2 m tiles carry -9999, written explicitly by shading-lu.nu's dem2m
# step. Luxembourg's terrain runs ~130-560 m so nothing legitimate approaches the
# sentinel, but -snodata is passed anyway to keep the behaviour identical to the
# other countries.
#
# Output: /media/martin/18TB/lu/luxembourg_contours.gpkg (layer `cont_lu_dtm`,
# EPSG:2169).
# Handoff to the splitter (split <=1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /media/martin/18TB/lu/luxembourg_contours.gpkg \
#       --source-table cont_lu_dtm --dest-table cont_lu_dtm_split \
#       --source-epsg 2169 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails with
# "unexpected argument". Omit it for fast Douglas-Peucker; pass it bare for Visvalingam.
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-lu.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/lu"
const SRC_DIR  = "/mnt/osm/lu/smooth2m"
const EPSG     = "EPSG:2169"

gdal require-proj $EPSG "contours-lu.nu"

gdal assert-mounted "/media/martin/18TB"

contours run {
    code:         "lu"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/luxembourg_dem_2m.vrt"
    dem_tif:      "/media/martin/18TB/lu/luxembourg_dem_2m.tif"
    gpkg:         "/media/martin/18TB/lu/luxembourg_contours.gpkg"
    table:        "cont_lu_dtm"                 # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  16384                            # PER PROCESS: fine for one pass, but raising
                                                   # off_interval runs up to 3 at once = 48 GB of 62.
                                                   # Drop to 2048 when you do.
}
