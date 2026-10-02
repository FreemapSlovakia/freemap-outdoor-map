#!/usr/bin/env nu

# Generate contour lines for Wallonia from the 2 m smoothed DEM tiles that
# shading-be-wal.nu emitted along the way. One half of a split Belgium; port of
# contours-be.nu, which covered both halves as a single table.
#
# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/be_wal/smooth2m is empty, run shading-be-wal.nu first.
#
# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.
#
# WHY THIS IS SPLIT FROM BELGIUM: the two halves come from sources under
#   different licences, and attribution.json covers BOTH shading and contours.
#   Splitting only the raster would still have a Flemish tile crediting Wallonia
#   for its contour lines. See shading-be-wal.nu.
#
# INTERVAL 10 m. Wallonia measures 50.0 m of relief per square kilometre with
#   0.4% of land under 10 m.
#   Wallonia is upland and 10 m will draw densely; there is no case for 5 m here.
#
# Source licence: CC BY 4.0, "Service public de Wallonie (SPW)".
#
# NODATA -9999, declared on every source raster. 0.00 is not a sentinel in
#   either half and nothing is masked anywhere in this pipeline.
#
# cachemax_mb is 2048, not the 16384 the unsplit Belgium used: parallel_off is 3,
#   so the larger figure reserves 48 GB of 62 and this box has had other work
#   holding 40 GB at a time.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-wallonia.nu

use lib/gdal.nu
use lib/contours.nu

const DATA_DIR = "/mnt/osm/be_wal"
const SRC_DIR  = "/mnt/osm/be_wal/smooth2m"
const EPSG     = "EPSG:3812"

gdal require-proj $EPSG "contours-wallonia.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "be_wal"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/wallonia_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/be_wal/wallonia_dem_2m.tif"
    gpkg:         $"($DRIVE)/be_wal/wallonia_contours.gpkg"
    table:        "cont_be_wal_dtm"            # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process — see header
}
