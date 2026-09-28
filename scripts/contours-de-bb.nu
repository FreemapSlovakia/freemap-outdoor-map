#!/usr/bin/env nu

# Generate contour lines for Brandenburg from the 2 m smoothed DEM tiles
# that shading-de-bb.nu emitted along the way.
# Port of contours-de-bw.nu. Output: <18TB>/de_bb/brandenburg_contours.gpkg,
# EPSG:25833.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_bb/smooth2m is empty, run shading-de-bb.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m, as everywhere except the Netherlands. The state runs from
# -50.4 m on the floor of the Rüdersdorf quarry to 201 m on the Kutschenberg,
# and its natural low is the Oderbruch at about sea level.
#
# MOST OF BRANDENBURG WILL CARRY ALMOST NO LINES AND THAT IS CORRECT. This is
# the flattest state in the set: roughness p50 0.019 over 31291 rasters, and
# median slope at the sample windows runs 1.2-4.3°. The Kutschenberg, the
# highest ground in the state, has 9.1 m of relief across a 2 km window, so it
# gets one contour. Do not reach for a 5 m interval to fill the gaps: the
# renderer's height filter draws multiples of 10 below z15, so the extra lines
# would roughly double the table while showing nothing at the zooms where flat
# country is actually viewed.

# DATUM: EPSG:25833 is ETRS89 / UTM 33N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard.

# NODATA is -9999 throughout, with no all-zero edge row of the kind Hessen hid
# in 17 rasters. 0.00 DOES occur, in 158 of the 31291 rasters, and it is REAL
# GROUND: the Oderbruch sits at and below sea level. Baden-Württemberg's
# `-srcnodata 0` applied here would punch holes in that floodplain.
#
# EXPECT CONTOURS AT AND BELOW ZERO, DOWN TO -50 m, AND DO NOT TREAT THEM AS
# DAMAGE. Three sources, all verified: the Rüdersdorf limestone quarry reaches
# -50.4 m, the Lausitz lignite workings near Jänschwalde -13.9 m, and the
# Oderbruch touches -0.04 m as ordinary farmland. Sachsen-Anhalt's open-cast
# floors were the same story at -12.4 m.
#
# LAKES AND THE ODER ARE FLAT SURFACES, not holes: lidar does not penetrate
# water, so they are interpolated — 478 rasters are 30%+ planar, one entirely
# so. A 2 m DEM of a lake therefore yields no contours across it, which is
# right, and the renderer draws water over the top anyway.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_bb/brandenburg_contours.gpkg \
#          --source-table cont_de_bb_dtm --dest-table contours_de_bb \
#          --source-epsg 25833 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
# The URL above is the UNIX SOCKET form, which peer-authenticates and needs no
# password at all. splitter-rs uses rust-postgres, which reads neither
# $PGPASSWORD nor ~/.pgpass, so a TCP URL would need the password inline. Pass
# --simplify-tolerance explicitly; the default of 1.0 silently loads a much
# denser table than the other countries.
#
# RESTORING ONTO fm5 — TWO TRAPS, BOTH MET IN PRODUCTION:
#
#   1. RUN pg_restore AS `freemap`, NOT AS postgres. `--no-owner` does not mean
#      "keep the owner", it means "give it to the restoring role". Restoring as
#      the postgres superuser produces a table owned by postgres with no ACL,
#      the renderer connects as `freemap`, and EVERY contour query fails with
#      permission denied — rendering breaks with no error in the journal. fm5
#      has a `freemap` OS user, so `sudo -u freemap` peer-authenticates and gets
#      the ownership right the first time.
#
#   2. PASS THE TABLESPACE. fm5 keeps contours in `contours_ts`, and
#      --no-tablespaces (needed because the dump carries this box's osm_ext,
#      which does not exist there) silently drops the table onto pg_default.
#
#      pg_dump "postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        --format=custom --compress=zstd --no-owner --no-privileges \
#        --table=public.contours_de_bb --file=contours_de_bb.dump
#      scp contours_de_bb.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_bb.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_bb WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete brandenburg_dem_2m.vrt, brandenburg_dem_2m.tif AND
# brandenburg_contours_off*.gpkg — otherwise the contour stage prints "skip
# offset N — already done" and republishes the previous run's vectors from a
# stale partial. This cost a full wasted cycle on the Netherlands; the giveaway
# was an output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-bb.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_bb"
const SRC_DIR  = "/mnt/osm/de_bb/smooth2m"
const EPSG     = "EPSG:25833"

gdal require-proj $EPSG "contours-de-bb.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "debb"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/brandenburg_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_bb/brandenburg_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_bb/brandenburg_contours.gpkg"
    table:        "cont_de_bb_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
