#!/usr/bin/env nu

# Contours for the Netherlands from the 2 m smoothed tiles shading-nl.nu emits.
# Port of contours-de-nw.nu. Output: <18TB>/nl/nl_contours.gpkg, EPSG:28992.

# Must come from the smoothed DEM: contouring raw lidar gives spaghetti, every
# furrow a closed loop. Luxembourg measured 46103 features unsmoothed against
# 30995 smoothed over identical terrain.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# Interval is 5 m, not the 10 m used elsewhere. Outside the Vaalserberg corner
# the country lies between about -7 m and +50 m, so 10 m would give almost
# nothing across the polders. Negative heights are real; do not floor them.

# Source is smooth2m-filled, not smooth2m. AHN5 keeps only maaiveld-classified
# returns, so vegetation leaves thin void slivers — 1 px median, 98% under 6 px —
# which gdal_contour breaks every crossing line at. Filling closes them while
# leaving open water and large structures as genuine nodata.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/nl/nl_contours.gpkg \
#          --source-table cont_nl_dtm --dest-table contours_nl \
#          --source-epsg 28992 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
# The splitter needs the password IN the URL: it uses rust-postgres, which reads
# neither $PGPASSWORD nor ~/.pgpass. Interpolating it as above keeps the secret
# out of this public repo and out of `ps`. Pass --simplify-tolerance explicitly;
# the default of 1.0 silently loads a denser table than the other countries.
#
# --simplify-high-quality and --drop-existing are boolean FLAGS.
#
# pg_restore onto fm5 needs --no-tablespaces (the dump carries this box's
# osm_ext) and --no-owner (no `martin` role there):
#
#      pg_restore --dbname=freemap --no-owner --no-privileges --no-tablespaces \
#        --single-transaction /tmp/contours_nl.dump

# Resumable: VRT, consolidated raster and GPKG are each skipped if present.
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-nl.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/nl"
const EPSG     = "EPSG:28992"

# TEMPORARY: this points at fill-smooth2m-nl.nu's ONE-OFF REPAIR directory, not
# at the shading run's own output. The tiles in smooth2m/ were written before
# shading-nl.nu emitted the 2 m DEM from the FILLED raster, so every building,
# tree clump and ditch is still a hole in them and gdal_contour breaks a line at
# each one. smooth2m-filled/ is a filled copy of those tiles; smooth2m/ itself
# was left untouched.
#
# A FRESH shading-nl.nu RUN WRITES CORRECT TILES TO smooth2m/. Once the country
# has been re-rendered, point this back at smooth2m/ — otherwise this silently
# contours the stale repaired set and ignores the new render.
#
# SWITCHING ALSO MEANS MOVING FOUR OUTPUTS ASIDE, because every stage is skipped
# when its output exists and all four were built from the old directory:
#   /mnt/osm/nl/nl_dem_2m.vrt
#   /mnt/osm/nl/nl_contours_off0.gpkg
#   <18TB>/nl/nl_dem_2m.tif
#   <18TB>/nl/nl_contours.gpkg
# Miss the GPKG and the run prints "exists — skipping" and changes nothing. Move
# ONLY the GPKG and it is worse: the pass finds the old off0 partial, skips it,
# and the merge copies that partial into a "new" GPKG identical to the old one.
# Rename rather than delete, as with the .unfilled copies from the last switch.
const SRC_DIR  = "/mnt/osm/nl/smooth2m-filled"

gdal require-proj $EPSG "contours-nl.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "nl"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/nl_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/nl/nl_dem_2m.tif"
    gpkg:         $"($DRIVE)/nl/nl_contours.gpkg"
    table:        "cont_nl_dtm"                 # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     5                                 # m — see header
    off_interval: 5
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
