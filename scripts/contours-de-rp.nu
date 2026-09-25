#!/usr/bin/env nu

# Generate contour lines for Rheinland-Pfalz from the 2 m smoothed DEM tiles
# that shading-de-rp.nu emitted along the way.
# Port of contours-de-bw.nu. Output: <18TB>/de_rp/rheinland_pfalz_contours.gpkg,
# EPSG:25832.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_rp/smooth2m is empty, run shading-de-rp.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m, as everywhere except the Netherlands. The state runs from the
# Rhine at about 60 m to 816 m on the Erbeskopf, so 10 m is unambiguous.
#
# THE RHINE PLAIN WILL CARRY ALMOST NO LINES AND THAT IS CORRECT. Its sample
# window is the flattest ground measured in any state — 1.4° median slope, and
# 0.44% of pixels changing between z16 and z17 — so large areas get one contour
# or none. Do not reach for a 5 m interval to fill them: the renderer's height
# filter draws multiples of 10 below z15, so the extra lines would roughly
# double the table while showing nothing at the zooms where the flat graben is
# actually viewed.

# DATUM: EPSG:25832 is ETRS89 / UTM 32N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard.

# NODATA is -9999 throughout and, uniquely among the German states here, no
# raster contains a literal 0.00 — scanned over all 21160. So there is nothing
# to undo: no `-srcnodata 0` as Baden-Württemberg needs for its undeclared
# out-of-coverage zeros, and no zero edge row as Hessen hid in 17 rasters.
#
# EXPECT CONTOURS DOWN TO ABOUT 20 m AND DO NOT TREAT THEM AS DAMAGE. The only
# sub-40 m ground in the state is two quarry floors — 36.7 m, and 12.2 m in the
# limestone pit near Diez that is benched down from 229 m. They are real.
#
# THE RHINE, MOSEL AND SAAR ARE FLAT SURFACES, not holes: lidar does not
# penetrate water, so they are interpolated — 50 rasters are 30%+ planar and
# every one is water. A 2 m DEM of a river therefore yields no contours across
# its width, which is right, and the renderer draws water over the top anyway.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_rp/rheinland_pfalz_contours.gpkg \
#          --source-table cont_de_rp_dtm --dest-table contours_de_rp \
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
#        --table=public.contours_de_rp --file=contours_de_rp.dump
#      scp contours_de_rp.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_rp.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_rp WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete rheinland_pfalz_dem_2m.vrt, rheinland_pfalz_dem_2m.tif AND
# rheinland_pfalz_contours_off*.gpkg — otherwise the contour stage prints "skip
# offset N — already done" and republishes the previous run's vectors from a
# stale partial. This cost a full wasted cycle on the Netherlands; the giveaway
# was an output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-rp.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_rp"
const SRC_DIR  = "/mnt/osm/de_rp/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-de-rp.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "derp"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/rheinland_pfalz_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_rp/rheinland_pfalz_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_rp/rheinland_pfalz_contours.gpkg"
    table:        "cont_de_rp_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
