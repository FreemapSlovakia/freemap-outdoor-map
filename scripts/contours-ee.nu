#!/usr/bin/env nu

# Generate contour lines for Estonia from the 2 m smoothed DEM tiles that
# shading-ee.nu emitted along the way.
# Port of contours-dk.nu, which shares the interval and the masked sea.
# Output: <18TB>/ee/estonia_contours.gpkg, EPSG:3301.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/ee/smooth2m is empty, run shading-ee.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 5 m, as in the Netherlands, Denmark and Flanders. Estonia is the
# flattest source in this repository: 59.9% of its land carries under 10 m of
# relief in a square kilometre, against Flanders' 51.6% and Denmark's 36.3%,
# and both of those moved to 5 m once 10 m was measured too sparse. Denmark
# delivered 4.71 km of line per km² at 10 m and 10.14 at 5 m; Flanders 4.83 and
# 10.14. Check the delivered density here too rather than assuming it landed in
# the same place.

# THE RENDERER DRAWS THE 5 m LINES WHERE THEY MATTER. Its height filter is
# `% 50` at z12, `% 20` at z13-14 and `% 5` from z15 up, so the extra lines
# appear at the zooms where flat country is examined closely and nowhere below.

# NODATA IS THE SEA, MASKED UPSTREAM. download-ee.nu puts `-srcnodata 0` on
# all.vrt, so the smoothed tiles arrive here with the sea already void. 0.00 is
# not real ground anywhere in this delivery — the split across the datum is
# 100:1 in favour of the positive side, which is a shoreline clamp rather than
# terrain. Do not mask it a second time.

# THE FLOOR IS THE OIL SHALE, AND THE SAMPLE DID NOT SEE IT. The 25 m national
# model holds 5099 cells below zero, to -13.58 m — the open-casts in the
# north-east, around 3.2 km². A 571-window scan covering about 1% of the country
# found nothing under -20 m, which says what is typical and nothing about the
# extreme. Expect a handful of negative lines around Kohtla-Järve and Narva;
# read this table, not the scan, for the real floor. Denmark and Wales both
# taught the same lesson, and in Wales the thing the sample missed was a defect.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/ee/estonia_contours.gpkg \
#          --source-table cont_ee_dtm --dest-table contours_ee \
#          --source-epsg 3301 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
# The URL above is the UNIX SOCKET form, which peer-authenticates and needs no
# password. splitter-rs uses rust-postgres, which reads neither $PGPASSWORD nor
# ~/.pgpass, so a TCP URL would need the password inline. Pass
# --simplify-tolerance explicitly; the default of 1.0 silently loads a much
# denser table than the other countries, which is what England had been doing.
#
# RESTORING ONTO fm5 — TWO TRAPS, BOTH MET IN PRODUCTION:
#
#   1. RUN pg_restore AS `freemap`, NOT AS postgres. `--no-owner` does not mean
#      "keep the owner", it means "give it to the restoring role". Restoring as
#      the postgres superuser produces a table owned by postgres with no ACL,
#      the renderer connects as `freemap`, and EVERY contour query fails with
#      permission denied — rendering breaks with no error in the journal.
#
#   2. PASS THE TABLESPACE. fm5 keeps contours in `contours_ts`, and
#      --no-tablespaces (needed because the dump carries this box's osm_ext,
#      which does not exist there) silently drops the table onto pg_default.
#
#      pg_dump "postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        --format=custom --compress=zstd --no-owner --no-privileges \
#        --table=public.contours_ee --file=contours_ee.dump
#      scp contours_ee.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_ee.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_ee WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete estonia_dem_2m.vrt, estonia_dem_2m.tif AND
# estonia_contours_off*.gpkg — otherwise the contour stage prints "skip offset N
# — already done" and republishes the previous run's vectors from a stale
# partial. This cost a full wasted cycle on the Netherlands; the giveaway was an
# output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-ee.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/ee"
const SRC_DIR  = "/mnt/osm/ee/smooth2m"
const EPSG     = "EPSG:3301"

gdal require-proj $EPSG "contours-ee.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "ee"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/estonia_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/ee/estonia_dem_2m.tif"
    gpkg:         $"($DRIVE)/ee/estonia_contours.gpkg"
    table:        "cont_ee_dtm"                 # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     5                                # m — see header
    off_interval: 5
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
