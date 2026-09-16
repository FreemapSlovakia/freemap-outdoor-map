#!/usr/bin/env nu

# Generate contour lines for all of Poland from the smoothed DTM tiles.
# Poland port of contours.nu (which targets Spain / UTM zones 29-31).
#
# Pipeline: smooth2m/*.tif (from shading.nu)
#                         → one national VRT (EPSG:2180)
#                         → consolidate to ONE raster on SSD
#                         → gdal_contour on that raster → GPKG (EPSG:2180)
#
# THE HEAL AND CROP STAGES ARE GONE, because shading.nu no longer retiles. It
#   emits smooth2m/ directly: each window already filled with gdal_fillnodata
#   (which is what dem_zero_speck_tiles.txt + the heal pass existed to do, only
#   now it happens to every window rather than to a hand-listed subset), already
#   cropped to its exact extent so there is no overlap to strip, and already at
#   2 m. dem_zero_speck_tiles.txt is no longer read by anything.
#
# Why consolidate before contouring: gdal_contour over the 143k-tile VRT is
# pathologically slow — scattered reads, tiles reopened per scanline. Merging the
# VRT into a single contiguous raster first (one HDD pass) lets the contour read
# sequentially off the NVMe. Spain warped here because it spanned 3 UTM zones that
# had to be unified into one CRS; Poland is a single CRS (2180), so no reprojection
# is needed, and no resampling either: the tiles arrive at 2 m, downsampled from
# the smoothed 1 m DEM with nodata-aware `average` while it was still in RAM
# (verified: bilinear/cubic blend the 0 nodata, average does not). The splitter
# reprojects the vectors to 3857 later (exact — no raster resampling error).
#
# Output: /mnt/osm/poland_contours.gpkg (layer `cont_pl_dmr`, EPSG:2180).
# Handoff to the splitter (split ≤1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/poland_contours.gpkg \
#       --source-table cont_pl_dmr --dest-table cont_pl_dmr_split \
#       --source-epsg 2180 --split-max-points 1000 \
#       --simplify-tolerance 2 --simplify-high-quality false --commit-interval 1000
#
# nodata: GUGiK's WCS returns 0 both for out-of-coverage AND for genuine 0.00 m
# coastal terrain (Żuławy). gdal_contour honours nodata, so interior 0 specks
# would notch the low contours — shading.nu's per-window gdal_fillnodata -md 5
# closes them before smooth2m/ is written. Large out-of-coverage 0 regions (sea)
# are far bigger than -md 5 can reach, so they stay nodata and are correctly
# excluded here by -snodata 0.
#
# Resumable: national VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Requires shading.nu's smooth2m/ populated. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-pl.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/run/media/martin/4TB/PL"
const SRC_DIR  = "/run/media/martin/4TB/PL/smooth2m"
const EPSG     = "EPSG:2180"                     # ETRF2000-PL / CS92

gdal require-proj $EPSG "contours-pl.nu"

contours run {
    code:         "pl"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/poland_dem_2m.vrt"
    dem_tif:      "/mnt/osm/poland_dem_2m.tif"     # consolidated DEM on SSD
    gpkg:         "/mnt/osm/poland_contours.gpkg"  # splitter input, fast SSD
    table:        "cont_pl_dmr"                    # layer name inside the GPKG
    height_col:   "height"
    nodata:       "0"                              # AMBIGUOUS BY DELIVERY — see header
    epsg:         $EPSG
    interval:     10
    off_interval: 10                               # one pass — see cachemax_mb before raising
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  16384                            # PER PROCESS: fine for one pass, but raising
                                                   # off_interval runs up to 3 at once = 48 GB of 62.
                                                   # Drop to 2048 when you do.
}
