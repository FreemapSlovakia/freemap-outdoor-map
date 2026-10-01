#!/usr/bin/env nu

# Generate contour lines for Denmark from the 2 m smoothed DEM tiles that
# shading-dk.nu emitted along the way.
# Port of contours-de-mv.nu. Output: <18TB>/dk/denmark_contours.gpkg,
# EPSG:25832.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/dk/smooth2m is empty, run shading-dk.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 5 m, as in the Netherlands and nowhere else. The country runs from
# -24.8 m in a Copenhagen excavation to 170.86 m at Møllehøj.
#
# TEN METRES LEAVES A THIRD OF THE COUNTRY BLANK. Measured over 630 mostly-land
# rasters drawn at random: 36.3% of them hold under 10 m of relief across their
# whole square kilometre, so at a 10 m interval they carry one line or none, and
# the median raster has 12.5 m of relief and carries one. Delivered at 10 m,
# Denmark came to 4.71 km of line per km² against the Netherlands' 7.42 at 5 m.
#
# THE RENDERER DRAWS THE 5 m LINES WHERE THEY MATTER. Its height filter is
# `% 50` at z12, `% 20` at z13-14 and `% 5` from z15 up, so the extra lines
# appear exactly at the zooms where flat country is examined closely, and
# nowhere below. Widths keep the hierarchy readable: 0.6 at multiples of 100,
# 0.3 at 10, 0.2 between. Labels are unaffected, being multiples of 50.

# DATUM: EPSG:25832 is ETRS89 / UTM 32N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard.

# NODATA IS THE SEA, MASKED UPSTREAM. download-dk.nu puts `-srcnodata 0` on
# all.vrt, so the smoothed tiles arrive here with the sea already void and there
# is nothing to undo. Do not mask 0.00 a second time.

# THE LOWEST LINES ARE -20 m AND THEY ARE MAN-MADE. Lammefjord is the lowest
# *natural* land at about -7.65 m and carries no negative contour at all. Every
# line below it is an excavation:
#
#     Stevns chalk quarry   25116 px below -15 m, floor -20.07 m, 2.5 ha
#     Limfjord pit            224 px, -23.91 m
#     Copenhagen, three sites 22-177 px each, -21.6 m to -24.8 m
#
#   34 features at -10 m and 6 at -20 m, out of 1153240. They are coherent
#   surfaces with internal relief (std 1.7-3.1 m), not single-pixel spikes, and
#   a 20-25 m excavation in ground at 0-20 m is ordinary. Do not mask them.
#
#   THE FLOOR IS NOT WHAT A SAMPLE SAYS IT IS. An 800-raster scan put the
#   country's minimum at -14.97 m and the expected lowest contour at -10 m;
#   both were wrong, because 800 of 50013 rasters is 1.6% and the deep points
#   here are each a fraction of a hectare. A sample bounds the typical, never
#   the extreme — for the extreme, read the contour table, which has seen every
#   pixel.

# INLAND WATER IS A FLAT SURFACE, NOT A HOLE: lakes carry their real surface
# height, so lidar's failure to penetrate leaves an interpolated plane that
# yields no contours across its width. That is right, and the renderer draws
# water over the top anyway. Only the sea itself is void.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/dk/denmark_contours.gpkg \
#          --source-table cont_dk_dtm --dest-table contours_dk \
#          --source-epsg 25832 --split-max-points 1000 \
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
#        --table=public.contours_dk --file=contours_dk.dump
#      scp contours_dk.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_dk.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_dk WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete denmark_dem_2m.vrt, denmark_dem_2m.tif AND
# denmark_contours_off*.gpkg — otherwise the contour stage prints "skip offset N
# — already done" and republishes the previous run's vectors from a stale
# partial. This cost a full wasted cycle on the Netherlands; the giveaway was an
# output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-dk.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/dk"
const SRC_DIR  = "/mnt/osm/dk/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-dk.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "dk"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/denmark_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/dk/denmark_dem_2m.tif"
    gpkg:         $"($DRIVE)/dk/denmark_contours.gpkg"
    table:        "cont_dk_dtm"                 # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     5                                # m — see header
    off_interval: 5
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
