#!/usr/bin/env nu

# Generate contour lines for all of Croatia from the smoothed 1 m DMR tiles.
# Croatia port of contours-pl.nu (Poland 1 m) / contours-it.nu (Italy 5 m).
#
# Pipeline: smooth2m/*.tif (from shading-hr.nu)
#             -> one national VRT (EPSG:3765)
#             -> consolidate to ONE raster on NVMe
#             -> gdal_contour on that raster -> GPKG (EPSG:3765)
#
# THE CROP AND THE DOWNSAMPLE ARE GONE, because shading-hr.nu no longer retiles.
#   It emits smooth2m/ directly: each window already cropped to its exact extent
#   (-projwin at the window bounds, collar excluded) and already at 2 m, resampled
#   with nodata-aware `average` while the smoothed DEM was still in RAM. So the
#   tiles tile the plane with no overlap and no gaps — there is no 6 px overlap
#   left to strip and no 1 m data left to halve. The consolidation pass stays.
#
# Why consolidate before contouring: gdal_contour over a 62k-tile VRT is pathologically
# slow (scattered reads, tiles reopened per scanline). Merging into a single contiguous
# raster first lets the contour read sequentially off NVMe. Croatia is a single CRS
# (3765), so no reprojection either — a plain gdal_translate. The splitter
# reprojects the vectors to 3857 later.
#
# nodata is clean: this reads the smooth2m/ tiles shading-hr.nu produced, whose
# source (hr.vrt) already unified the provider's mixed sentinels (-3.4e38 / -99 /
# -32767 / 0) to a single -9999. So NONE of Poland's zero-speck heal step is needed —
# that existed only because GUGiK's WCS overloaded 0 for out-of-coverage. Here -snodata
# -9999 cleanly excludes out-of-coverage and keeps genuine 0.00 m coastal contours.
#
# Output: /mnt/osm/hr/croatia_contours.gpkg (layer `cont_hr_dmr`, EPSG:3765).
# Handoff to the splitter (split <=1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/hr/croatia_contours.gpkg \
#       --source-table cont_hr_dmr --dest-table cont_hr_dmr_split \
#       --source-epsg 3765 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails with
# "unexpected argument". Omit it for fast Douglas-Peucker; pass it bare for Visvalingam.
#
# Resumable: the national VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Requires shading-hr.nu's smooth2m/ populated. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-hr.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/hr"
const SRC_DIR  = "/mnt/osm/hr/smooth2m"
const EPSG     = "EPSG:3765"

gdal require-proj $EPSG "contours-hr.nu"

contours run {
    code:         "hr"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/croatia_dem_2m.vrt"
    dem_tif:      $"($DATA_DIR)/croatia_dem_2m.tif"
    gpkg:         $"($DATA_DIR)/croatia_contours.gpkg"
    table:        "cont_hr_dmr"                    # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10                               # one pass — see cachemax_mb before raising
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  16384                            # PER PROCESS: fine for one pass, but raising
                                                   # off_interval runs up to 3 at once = 48 GB of 62.
                                                   # Drop to 2048 when you do.
}
