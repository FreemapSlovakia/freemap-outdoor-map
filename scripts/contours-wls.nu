#!/usr/bin/env nu

# Generate contour lines for Wales from the 2 m smoothed DEM tiles that
# shading-wls.nu emitted along the way.
# Port of contours-en.nu, which shares the datum and the instrument.
# Output: <18TB>/wls/wales_contours.gpkg, EPSG:27700.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/wls/smooth2m is empty, run shading-wls.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m, and there is no case here for the 5 m used in the Netherlands
# and Denmark. Those two are flat: Denmark has 36.3% of its land under 10 m of
# relief per square kilometre and a median of 12.5 m, so a 10 m interval left a
# third of it blank. Wales measured 2.2% and a median of 107.6 m over 775 random
# land windows — it is the steepest source in this repository, and 10 m will
# already draw densely. Check the delivered density before reaching for 5 m:
# Denmark came to 4.71 km of line per km² at 10 m against the Netherlands' 7.42
# at 5 m, and anything near the latter needs no help.

# DATUM: EPSG:27700 IS ON OSGB36 AND THIS PRODUCT IS CORRECT. OSTN15 has been
# installed box-wide since 2026-08-17, so the reprojection to Web Mercator uses
# the real NTv2 grid rather than the 7-parameter Helmert that PROJ falls back to
# in its absence — which across Wales is 0.61-1.97 m out.
#
#   ENGLAND IS NOT CORRECT, AND THE TWO WILL NOT AGREE. england_contours and its
#   shading were built before OSTN15 was installed and carry ~1.9 m of error. At
#   the border expect contour lines that do not quite join and shaded features a
#   pixel or so apart — about 1.3 px at z16 over 250 km. That is England owing a
#   rebuild, not Wales being wrong; see the datum section of contours-en.nu,
#   which costs ~17 h of shading plus ~2 h of splitter and needs no setup.

# NODATA IS THE SEA AND THE BORDER, DECLARED HONESTLY as -9999 and covering
# 51.3% of the source rectangle. 0.00 is real ground — 28 of 775 sampled land
# windows carry zeros and the sub-datum ground is written as a negative — so
# nothing is masked anywhere in this pipeline. Do not add a mask here.

# TWO BAD PIXELS NEAR CAERNARFON, AND THEY MUST BE DELETED AFTER LOADING.
# At 53.13169,-4.37974 the water surface of Foryd Bay sits at a median -2.13 m
# over 21338 px, and inside it are exactly two outliers: one at -75.04 m and one
# in the -20s. They produce five contour features — the only ones in the country
# below -20 m — each 10 to 23 m long. Run this against the loaded table:
#
#     DELETE FROM contours_wls
#      WHERE height_m <= -10
#        AND ST_DWithin(wkb_geometry,
#              ST_Transform(ST_SetSRID(ST_MakePoint(-4.37974,53.13169),4326),3857), 100);
#
#   THE RADIUS MATTERS: the Margam opencast carries a genuine -10 m line 1899 m
#   long, 305 km away, and must survive. A blanket `height_m <= -10` would take
#   it. After the delete the floor is -10 m, in three features, all Margam's.
#
#   The source COG is left byte-exact on purpose — download-wls.nu verifies it
#   against the server's Content-Length, and rewriting two pixels in a DEFLATE
#   COG changes block sizes, so a patched file would be re-fetched in full,
#   45 GB, on the next run. Two pixels are also invisible in the shading: at
#   z16's 2.39 m they do not fill one output pixel.
#
# A 3.7% SAMPLE DID NOT FIND THEM, AND COULD NOT. 775 random land windows
# reported nothing below -20 m and were right about every window they saw; the
# defect is two pixels in 20780 km². The contour table has seen every pixel, so
# it is the authority on the floor — the same lesson Denmark taught, where a
# sampled minimum of -14.97 m missed real ground at -24.8 m.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/wls/wales_contours.gpkg \
#          --source-table cont_wls_dtm --dest-table contours_wls \
#          --source-epsg 27700 --split-max-points 1000 \
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
#      permission denied — rendering breaks with no error in the journal.
#
#   2. PASS THE TABLESPACE. fm5 keeps contours in `contours_ts`, and
#      --no-tablespaces (needed because the dump carries this box's osm_ext,
#      which does not exist there) silently drops the table onto pg_default.
#
#      pg_dump "postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        --format=custom --compress=zstd --no-owner --no-privileges \
#        --table=public.contours_wls --file=contours_wls.dump
#      scp contours_wls.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_wls.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_wls WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete wales_dem_2m.vrt, wales_dem_2m.tif AND
# wales_contours_off*.gpkg — otherwise the contour stage prints "skip offset N —
# already done" and republishes the previous run's vectors from a stale partial.
# This cost a full wasted cycle on the Netherlands; the giveaway was an output
# byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-wls.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/wls"
const SRC_DIR  = "/mnt/osm/wls/smooth2m"
const EPSG     = "EPSG:27700"

gdal require-proj $EPSG "contours-wls.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "wls"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/wales_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/wls/wales_dem_2m.tif"
    gpkg:         $"($DRIVE)/wls/wales_contours.gpkg"
    table:        "cont_wls_dtm"                # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
