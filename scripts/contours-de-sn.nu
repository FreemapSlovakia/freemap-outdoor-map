#!/usr/bin/env nu

# Generate contour lines for Sachsen from the 2 m smoothed DEM tiles that
# shading-de-sn.nu emitted along the way.
# Saxony port of contours-de-nw.nu. Output: <18TB>/de_sn/sachsen_contours.gpkg,
# EPSG:25833.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_sn/smooth2m is empty, run shading-de-sn.nu first.

# The consolidation pass is not skipped — gdal_contour over a several-thousand
# tile VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m, not the Netherlands' 5 m. Saxony runs from ~93 m on the Elbe to
# 1215 m at Fichtelberg, so 10 m is the right density and matches every other
# country; 5 m is a flat-country special case.

# DATUM: EPSG:25833 is ETRS89 / UTM 33N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard. Note the zone is
# 33N, unlike Bayern and NRW at 32N.

# NODATA is -9999, written explicitly by shading-de-sn.nu's dem2m step. Saxon
# terrain never approaches the sentinel.
#
# Large voids are EXPECTED and must stay: the state boundary, big water, and the
# Lusatian open-cast mines. shading-de-sn.nu deliberately runs with fill_md 0
# because the source has no interior voids to repair (measured: 141 interior
# windows, all exactly 0.0000% nodata). -snodata correctly excludes them here.
#
# CONTOURS BELOW 60 m ARE REAL, NOT ARTEFACTS. Saxony's lowest natural ground is
# about 75 m, but the run produced nested rings from -10 m up at 51.3827,
# 12.7659 — the Leipzig lignite open-cast pit, which is man-made and genuinely
# below sea level. Do not "fix" them.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_sn/sachsen_contours.gpkg \
#          --source-table cont_de_sn_dtm --dest-table contours_de_sn \
#          --source-epsg 25833 --split-max-points 1000 \
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
#      the ownership right the first time. If it is restored as postgres anyway:
#          ALTER TABLE public.contours_de_sn OWNER TO freemap;
#
#   2. PASS THE TABLESPACE. fm5 keeps contours in `contours_ts`, and
#      --no-tablespaces (needed because the dump carries this box's osm_ext,
#      which does not exist there) silently drops the table onto pg_default.
#
#      pg_dump "postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        --format=custom --compress=zstd --no-owner --no-privileges \
#        --table=public.contours_de_sn --file=contours_de_sn.dump
#      scp contours_de_sn.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_sn.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_sn WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete sachsen_dem_2m.vrt, sachsen_dem_2m.tif AND
# sachsen_contours_off*.gpkg — otherwise the contour stage prints "skip offset N
# — already done" and republishes the previous run's vectors from a stale
# partial. This cost a full wasted cycle on the Netherlands; the giveaway was an
# output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-sn.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_sn"
const SRC_DIR  = "/mnt/osm/de_sn/smooth2m"
const EPSG     = "EPSG:25833"

gdal require-proj $EPSG "contours-de-sn.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "desn"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/sachsen_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_sn/sachsen_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_sn/sachsen_contours.gpkg"
    table:        "cont_de_sn_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
