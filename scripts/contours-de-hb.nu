#!/usr/bin/env nu

# Generate contour lines for Bremen from the 2 m smoothed DEM tiles
# that shading-de-hb.nu emitted along the way.
# Port of contours-de-bw.nu. Output: <18TB>/de_hb/bremen_contours.gpkg,
# EPSG:25832.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_hb/smooth2m is empty, run shading-de-hb.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m. Bremen runs from about -12 m in the dock basins and the
# dredged Weser fairway to 48 m on the Geest edge, so the whole state fits in
# seven levels — the shallowest range of any state here.

# EXPECT A DENSE 0 m LINE, as in Hamburg and Schleswig-Holstein: the Weser
# marsh sits within a metre of sea level over much of both cities.

# DATUM: EPSG:25832 is ETRS89 / UTM 33N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard.

# NODATA IS -9999 AND MEANS "NO TILE", NOT "NO DATA HERE". The delivery
# declares no nodata and writes none — every tile is a full million points — so
# all.vrt sets -vrtnodata purely to fill ground no tile covers. -9999 appears
# nowhere in the sources.
#
# 0.00 IS REAL GROUND: 128 of 583 rasters carry zeros and not one is wholly
# zero. A -srcnodata 0 would delete the marsh.
#
# THE TWO CITIES WERE SURVEYED TWO YEARS APART and their archives disagreed
# about coordinates — see download-de-hb.nu. If the DEM ever looks displaced,
# check there first.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_hb/bremen_contours.gpkg \
#          --source-table cont_de_hb_dtm --dest-table contours_de_hb \
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
#        --table=public.contours_de_hb --file=contours_de_hb.dump
#      scp contours_de_hb.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_hb.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_hb WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete bremen_dem_2m.vrt, bremen_dem_2m.tif AND
# bremen_contours_off*.gpkg — otherwise the contour stage prints "skip
# offset N — already done" and republishes the previous run's vectors from a
# stale partial. This cost a full wasted cycle on the Netherlands; the giveaway
# was an output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-hb.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_hb"
const SRC_DIR  = "/mnt/osm/de_hb/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-de-hb.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "dehb"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/bremen_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_hb/bremen_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_hb/bremen_contours.gpkg"
    table:        "cont_de_hb_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
