#!/usr/bin/env nu

# Generate contour lines for Mecklenburg-Vorpommern from the 2 m smoothed DEM tiles
# that shading-de-mv.nu emitted along the way.
# Port of contours-de-bw.nu. Output: <18TB>/de_mv/mecklenburg_vorpommern_contours.gpkg,
# EPSG:25833.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_mv/smooth2m is empty, run shading-de-mv.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m, as everywhere except the Netherlands. The state runs from
# about -3.2 m in the coastal polders to 179 m on the Helpter Berge, so the
# whole state fits in nineteen levels.

# MOST OF MECKLENBURG-VORPOMMERN WILL CARRY ALMOST NO LINES AND THAT IS CORRECT.
# Roughness p50 is 0.020 over 6407 rasters, within a whisker of Brandenburg's
# 0.019, and the Müritz sample window has 1.9 m of relief across two
# kilometres. The exception is the Baltic coast: Rügen and Jasmund carry 60-80 m
# of relief in a single window and will be dense. Do not reach for a 5 m
# interval to fill the inland gaps — the renderer's height filter draws
# multiples of 10 below z15, so the extra lines would roughly double the table
# while showing nothing at the zooms where flat country is actually viewed.

# DATUM: EPSG:25833 is ETRS89 / UTM 33N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard.

# NODATA IS THE BALTIC, MASKED UPSTREAM. The delivery declares no nodata at all
# and writes the sea as a flat 0.00 surface — 941 of 6407 rasters contain zeros
# and several are entirely zero. download-de-mv.nu puts `-srcnodata 0` on
# all.vrt, so shading-de-mv.nu's smoothed tiles arrive here with the sea already
# void and there is nothing to undo. Do not mask 0.00 a second time.
#
# EXPECT NO CONTOUR BELOW ABOUT -10 m. The lowest ground in the state is around
# -3.2 m in coastal polders near the Bodden, so a 0 m line is real and a -10 m
# line is the floor. Nothing here resembles Brandenburg's -50 m quarry: the
# largest relief in any single raster is 72.4 m, on Rügen.
#
# THE MÜRITZ AND THE BODDEN ARE FLAT SURFACES, not holes: inland water carries
# its real surface height, the Müritz at about 62 m, so lidar's failure to
# penetrate leaves an interpolated plane that yields no contours across its
# width. That is right, and the renderer draws water over the top anyway. Only
# the Baltic itself is void.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_mv/mecklenburg_vorpommern_contours.gpkg \
#          --source-table cont_de_mv_dtm --dest-table contours_de_mv \
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
#        --table=public.contours_de_mv --file=contours_de_mv.dump
#      scp contours_de_mv.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_mv.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_mv WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete mecklenburg_vorpommern_dem_2m.vrt, mecklenburg_vorpommern_dem_2m.tif AND
# mecklenburg_vorpommern_contours_off*.gpkg — otherwise the contour stage prints "skip
# offset N — already done" and republishes the previous run's vectors from a
# stale partial. This cost a full wasted cycle on the Netherlands; the giveaway
# was an output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-mv.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_mv"
const SRC_DIR  = "/mnt/osm/de_mv/smooth2m"
const EPSG     = "EPSG:25833"

gdal require-proj $EPSG "contours-de-mv.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "demv"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/mecklenburg_vorpommern_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_mv/mecklenburg_vorpommern_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_mv/mecklenburg_vorpommern_contours.gpkg"
    table:        "cont_de_mv_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
