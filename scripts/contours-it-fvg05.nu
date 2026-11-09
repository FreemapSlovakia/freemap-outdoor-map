#!/usr/bin/env nu

# Contour lines for Friuli from the 0.5 m smoothed DEM that
# shading-it-fvg05.nu emitted along the way.
#
# ITS OWN TABLE, NOT PART OF A NATIONAL cont_it_dtm. Italy ships as eighteen
# datasets because eighteen authorities hold the sources; the renderer masks
# each one by the finer datasets above it in the stack, contours included, so
# lines never double up where coverage overlaps.
#
# INTERVAL 10 m, which is what Italy already carries. The 5 m interval belongs
# to the flat countries — the Netherlands, Denmark, Flanders, Estonia — where
# 10 m left the map empty. Italy is the opposite case.
#
# Must come from the smoothed DEM: contouring raw lidar gives spaghetti, every
# furrow and forest-floor speck a closed loop. If /mnt/osm/it_fvg05/smooth2m is
# empty, run shading-it-fvg05.nu first.
#
# THE CONSOLIDATION PASS IS NOT SKIPPED. gdal_contour over a many-thousand-tile
# VRT is pathologically slow: scattered reads, tiles reopened per scanline.
#
# THE SOURCE CARRIES DEFECTS AND THEY MUST BE REMOVED, NOT CLAMPED. out/fvg05.tif
# holds patches of impossible heights with no nodata marking them — one 20x20
# window in Friuli ranges -30310 m to +595492 m. A single such pixel makes
# gdal_contour emit a tiny ring at every level between it and the ground
# around it, so a height filter on the output cannot separate those rings from
# a real summit. valid_range drops them to nodata before contouring.
#
# NODATA IS -2147483648, NOT -9999. The Italian sources store heights as Int32
# in 2 mm steps and the shading run unscales them to metres; the nodata pixels
# keep that sentinel. Tell gdal_contour -9999 and it excludes nothing, sees a
# range of 214 million levels and refuses to start.
#
# EPSG:3857 BECAUSE THE SOURCE ALREADY IS. Every Italian source was warped onto
# the XYZ grid before any of this, so there is no national CRS here and
# splitter-rs takes --source-epsg 3857.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-it-fvg05.nu

use lib/gdal.nu
use lib/contours.nu

const DATA_DIR = "/mnt/osm/it_fvg05"
const SRC_DIR  = "/mnt/osm/it_fvg05/smooth2m"
const EPSG     = "EPSG:3857"

gdal require-proj $EPSG "contours-it-fvg05.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "it_fvg05"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/fvg05_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/it/fvg05_dem_2m.tif"
    gpkg:         $"($DRIVE)/it/fvg05_contours.gpkg"
    table:        "cont_it_fvg05_dtm"
    height_col:   "height"
    nodata:       "-2147483648"                    # the source's own sentinel — see below
    epsg:         $EPSG
    valid_range:  {min: -50, max: 3000}             # source defects — see header
    interval:     10
    off_interval: 10                               # one pass
    parallel_off: 3
    cachemax_mb:  4096                             # PER PROCESS; three passes would be 12 GB of 62
}
