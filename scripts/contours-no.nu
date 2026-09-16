#!/usr/bin/env nu

# Generate contour lines for all of Norway from the 2 m smoothed DEM tiles that
# shading-no.nu emitted along the way.
# Norway port of contours-hr.nu (Croatia 1 m) / contours-pl.nu (Poland 1 m).
#
# Pipeline: /mnt/osm/no/smooth2m/*.tif   (48101 tiles, 1500x1500 px, 2 m, EPSG:25833)
#             -> one national VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:25833)
#
# TWO DIFFERENCES FROM THE OLD RETILE-BASED contours-hr.nu, both because shading-no.nu
# did the work up front. (contours-hr.nu has since been converted and now works the
# same way; these notes record why the approach was adopted.)
#
#  * NO per-tile cropped VRTs. The Croatian script had to strip a 6 px overlap
#    from every smooth/ tile with its own gdal_translate -srcwin, 62351 of them.
#    shading-no.nu's dem2m step already wrote each window cropped to its exact
#    3 km extent (-projwin at the window bounds, collar excluded), so these tiles
#    tile the plane seamlessly with no overlap and no gaps. gdalbuildvrt is enough.
#
#  * NO 1 m -> 2 m downsampling. The tiles are already 2 m, produced with
#    nodata-aware `average` from the smoothed 1 m DEM while it was still in RAM.
#    That is the expensive half of Croatia's consolidation pass (8.5 h there;
#    it would have been ~45 h over Norway's area) already paid.
#
#    NOTE: the consolidation itself is NOT skipped — earlier notes of mine said
#    so and that was wrong. gdal_contour over a 48k-tile VRT is pathologically
#    slow (scattered reads, tiles reopened per scanline), so the tiles still have to be merged into one contiguous
#    raster first. What changed is that this pass now only copies 2 m data
#    instead of reading ~900 GB of 1 m data and resampling it.
#
# Consolidated DEM goes on the 18TB, not the NVMe: at Norway's bbox it is a few
# hundred GB and /mnt/osm has ~485 GB free with smooth2m already occupying 186 GB.
# gdal_contour reads it sequentially, so spinning rust costs little.
#
# nodata is clean: -9999 on every tile, presented unchanged by the VRT. Genuine
# 0.00 m coastal contours are preserved because out-of-coverage is -9999, never 0.
#
# Output: /mnt/osm/no/norway_contours.gpkg (layer `cont_no_dmr`, EPSG:25833).
# Handoff to the splitter (split <=1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/no/norway_contours.gpkg \
#       --source-table cont_no_dmr --dest-table cont_no_dmr_split \
#       --source-epsg 25833 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails with
# "unexpected argument". Omit it for fast Douglas-Peucker; pass it bare for Visvalingam.
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-no.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/no"
const SRC_DIR  = "/mnt/osm/no/smooth2m"
const EPSG     = "EPSG:25833"

gdal require-proj $EPSG "contours-no.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "no"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/norway_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/no/norway_dem_2m.tif"
    gpkg:         "/mnt/osm/no/norway_contours.gpkg"
    table:        "cont_no_dmr"                 # layer name inside the GPKG
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
