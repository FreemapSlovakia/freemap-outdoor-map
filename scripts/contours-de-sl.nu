#!/usr/bin/env nu

# Generate contour lines for Saarland from the 2 m smoothed DEM tiles
# that shading-de-sl.nu emitted along the way.
# Port of contours-de-bw.nu. Output: <18TB>/de_sl/saarland_contours.gpkg,
# EPSG:25832.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_sl/smooth2m is empty, run shading-de-sl.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m, as everywhere except the Netherlands. Saarland runs from about
# 140 m on the Saar at Saarlouis to 695 m on the Dollberg in the Schwarzwälder
# Hochwald, so the whole state fits in fifty-six levels — the deepest range of
# the six states added in this run.

# THIS STATE ACTUALLY HAS RELIEF. Roughness p50 is 0.045 over 2775 rasters,
# more than double Brandenburg's 0.019, and single tiles carry 221 m of it at
# the Saarschleife. Unlike the marsh states there is no dense zero line to
# expect and no reason to reach for a finer interval.

# DATUM: EPSG:25832 is ETRS89 / UTM 33N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard.

# NODATA IS AN HONEST -9999, declared on every raster and used for nothing
# else. Scanned over all 2775: exactly one raster contains 0.00 at all, 105
# pixels of it, and none holds ground below -2 m. There is no sentinel to undo
# here — no Baden-Württemberg 0.00, no Hamburg second marker, no Bremen
# coordinate repair. Do not add -srcnodata anything.
#
# EXPECT NO CONTOUR BELOW ZERO. The state's floor is about 140 m, so a line
# under 100 m would mean something is wrong.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_sl/saarland_contours.gpkg \
#          --source-table cont_de_sl_dtm --dest-table contours_de_sl \
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
#        --table=public.contours_de_sl --file=contours_de_sl.dump
#      scp contours_de_sl.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_sl.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_sl WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete saarland_dem_2m.vrt, saarland_dem_2m.tif AND
# saarland_contours_off*.gpkg — otherwise the contour stage prints "skip
# offset N — already done" and republishes the previous run's vectors from a
# stale partial. This cost a full wasted cycle on the Netherlands; the giveaway
# was an output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-sl.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_sl"
const SRC_DIR  = "/mnt/osm/de_sl/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-de-sl.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "desl"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/saarland_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_sl/saarland_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_sl/saarland_contours.gpkg"
    table:        "cont_de_sl_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
