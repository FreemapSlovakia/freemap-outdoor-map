#!/usr/bin/env nu

# Generate contour lines for Thüringen from the 2 m smoothed DEM tiles that
# shading-de-th.nu emitted along the way.
# Thuringian port of contours-de-sn.nu. Output:
# <18TB>/de_th/thueringen_contours.gpkg, EPSG:25832.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_th/smooth2m is empty, run shading-de-th.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m. Thüringen runs from about 114 m on the Unstrut to 982 m at the
# Großer Beerberg, so 10 m is the right density and matches every country except
# the Netherlands, whose 5 m is a flat-country special case.

# DATUM: EPSG:25832 is ETRS89 / UTM 32N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard. Same zone as
# Bayern and NRW; Saxony is the odd one at 33N.

# NODATA is -9999, written explicitly by shading-de-th.nu's dem2m step.
#
# GENUINE 0 m DOES NOT OCCUR — the lowest ground in the state is 113.55 m, as
# measured across the source. That matters because the 2014-2019 half of the
# source arrived as xyz whose unlisted cells read 0; download-de-th.nu rewrites
# those to -9999 (10.7 million cells, 60.23% of the 181 converted tiles). If a
# 0 m contour ever appears here, that rewrite has regressed — do not paper over
# it with -snodata 0, which would be Poland's fix for a different disease.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_th/thueringen_contours.gpkg \
#          --source-table cont_de_th_dtm --dest-table contours_de_th \
#          --source-epsg 25832 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
# The URL above is the UNIX SOCKET form, which peer-authenticates and needs no
# password at all — preferable to interpolating $PGPASSWORD, which puts the
# secret in the environment and in `ps`. splitter-rs uses rust-postgres, which
# reads neither $PGPASSWORD nor ~/.pgpass, so a TCP URL would need the password
# inline. Pass --simplify-tolerance explicitly; the default of 1.0 silently
# loads a much denser table than the other countries.
#
# --simplify-high-quality and --drop-existing are boolean FLAGS.
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
#        --table=public.contours_de_th --file=contours_de_th.dump
#      scp contours_de_th.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_th.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_th WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete thueringen_dem_2m.vrt, thueringen_dem_2m.tif AND
# thueringen_contours_off*.gpkg — otherwise the contour stage prints "skip
# offset N — already done" and republishes the previous run's vectors from a
# stale partial. This cost a full wasted cycle on the Netherlands; the giveaway
# was an output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-th.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_th"
const SRC_DIR  = "/mnt/osm/de_th/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-de-th.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "deth"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/thueringen_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_th/thueringen_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_th/thueringen_contours.gpkg"
    table:        "cont_de_th_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
