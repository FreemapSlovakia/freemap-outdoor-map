#!/usr/bin/env nu

# Generate contour lines for all of Italy from the national 5 m HRDTM.
# Italy port of contours-pl.nu (which targets Poland's 1 m GUGiK DTM).
#
# Pipeline: smooth2m/*.tif (from shading-it.nu, 5 m — see below)
#             -> one national VRT (EPSG:6875)
#             -> consolidate to ONE raster on NVMe
#             -> gdal_contour on that raster -> GPKG (EPSG:6875)
#
# IT CONTOURS THE SMOOTHED DEM, NOT it.tif. Contouring the raw source is one
#   command and needs no staging, but the lines come out wigglier: it.tif carries
#   LiDAR speckle plus, in places, corduroy striping and TIN facets from contour
#   interpolation. Smoothing moves the surface only ~0.4 m mean / 2 m p99, far
#   below the 10 m interval, so this changes line SHAPE, not accuracy. The old
#   USE_SMOOTH toggle is gone with the retile stage it selected between.
#
# NO CROP STAGE. shading-it.nu no longer retiles with an overlap, so there is no
#   6 px disagreement left to strip: each smooth2m/ tile is already cropped to
#   its exact window extent, collar excluded.
#
# NO DOWNSAMPLING, and the tiles are 5 m not 2 m. Poland went 1 m -> 2 m for its
#   contour pass, but 5 m is already the right density for a 10 m interval, so
#   shading-it.nu writes smooth2m/ at the source resolution. (The directory is
#   called smooth2m/ in every country for consistency; here it holds 5 m tiles.)
#
# nodata is -9999 throughout. Poland's `-snodata 0` MUST NOT be carried over: it
# would treat genuine 0.00 m coastal terrain as nodata and notch every low contour.
# Poland needed it only because its WCS overloaded 0 for out-of-coverage.
#
# Output: /mnt/osm/it/italy_contours.gpkg (layer `cont_it_dtm`, EPSG:6875).
# Handoff to the splitter (split ≤1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/it/italy_contours.gpkg \
#       --source-table cont_it_dtm --dest-table cont_it_dtm_split \
#       --source-epsg 6875 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value
# (`--simplify-high-quality false`, as contours-pl.nu's header does) fails with
# "unexpected argument 'false'". Omit it for the fast Douglas-Peucker default;
# pass it bare to opt into Visvalingam.
#
# Resumable: the national VRT, consolidated raster and GPKG are each skipped if
# present (delete to force a rebuild). Requires shading-it.nu's smooth2m/
# populated. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-it.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/it"
const SRC_DIR  = "/mnt/osm/it/smooth2m"          # 5 m tiles — see header
const EPSG     = "EPSG:6875"

gdal require-proj $EPSG "contours-it.nu"

contours run {
    code:         "it"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/italy_dem_5m.vrt"
    dem_tif:      $"($DATA_DIR)/italy_dem_5m.tif"
    gpkg:         $"($DATA_DIR)/italy_contours.gpkg"
    table:        "cont_it_dtm"                    # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"                          # NOT 0 — see header
    epsg:         $EPSG
    interval:     10
    off_interval: 10                               # one pass — see cachemax_mb before raising
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  16384                            # PER PROCESS: fine for one pass, but raising
                                                   # off_interval runs up to 3 at once = 48 GB of 62.
                                                   # Drop to 2048 when you do.
}
