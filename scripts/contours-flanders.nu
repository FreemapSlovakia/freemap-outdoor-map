#!/usr/bin/env nu

# Generate contour lines for Flanders from the 2 m smoothed DEM tiles that
# shading-be-vlg.nu emitted along the way. One half of a split Belgium; port of
# contours-be.nu, which covered both halves as a single table.
#
# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/be_vlg/smooth2m is empty, run shading-be-vlg.nu first.
#
# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.
#
# WHY THIS IS SPLIT FROM BELGIUM: the two halves come from sources under
#   different licences, and attribution.json covers BOTH shading and contours.
#   Splitting only the raster would still have a Flemish tile crediting Wallonia
#   for its contour lines. See shading-be-vlg.nu.
#
# INTERVAL 5 m, AS IN THE NETHERLANDS AND DENMARK AND NOWHERE ELSE. Flanders
#   measures 9.9 m of relief per square kilometre with 51.6% of land under 10 m,
#   flatter than Denmark. Delivered at 10 m it came to 4.83 km of line per km2,
#   within a whisker of the 4.71 that moved Denmark to 5 m, and well under the
#   Netherlands' 7.42. Wallonia stays at 10 m: it delivers 9.70.
#
#   THE RENDERER DRAWS THE EXTRA LINES WHERE THEY MATTER. Its height filter is
#   `% 50` at z12, `% 20` at z13-14 and `% 5` from z15 up, so they appear at the
#   zooms where flat country is examined closely and nowhere below.
#
#   (superseded) At 9.9 m of relief per square kilometre with 51.6% of land under 10 m, it is flatter than Denmark, which was moved to a 5 m interval after 10 m delivered only 4.71 km of line per km2 against the Netherlands 7.42 at 5 m. This runs at 10 m to match what Belgium already carried; measure the delivered density before deciding, because that is the number that settled Denmark.
#
# Source licence: "Modellicentie gratis hergebruik", Digitaal Vlaanderen.
#
# NODATA -9999, declared on every source raster. 0.00 is not a sentinel in
#   either half and nothing is masked anywhere in this pipeline.
#
# cachemax_mb is 2048, not the 16384 the unsplit Belgium used: parallel_off is 3,
#   so the larger figure reserves 48 GB of 62 and this box has had other work
#   holding 40 GB at a time.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-flanders.nu

use lib/gdal.nu
use lib/contours.nu

const DATA_DIR = "/mnt/osm/be_vlg"
const SRC_DIR  = "/mnt/osm/be_vlg/smooth2m"
const EPSG     = "EPSG:3812"

gdal require-proj $EPSG "contours-flanders.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "be_vlg"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/flanders_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/be_vlg/flanders_dem_2m.tif"
    gpkg:         $"($DRIVE)/be_vlg/flanders_contours.gpkg"
    table:        "cont_be_vlg_dtm"            # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     5                                # m — see header
    off_interval: 5
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process — see header
}
