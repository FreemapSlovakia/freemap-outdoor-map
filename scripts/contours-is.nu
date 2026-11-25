#!/usr/bin/env nu

# Generate contour lines for Iceland from the 10 m smoothed DEM tiles that
# shading-is.nu emitted along the way. Port of contours-it-puglia8.nu, the other
# coarse source. Output: <18TB>/is/is_contours.gpkg, EPSG:3057.
#
# Must come from the smoothed DEM: shading-is.nu's smooth2m tiles are the
# feature-preserving-smoothed window at 10 m in ISN93. If /mnt/osm/is/smooth2m
# is empty, run shading-is.nu first.
#
# The consolidation pass is not skipped — gdal_contour over a many-tile VRT is
# pathologically slow, reopening tiles per scanline.
#
# INTERVAL 10 m, as everywhere except the Netherlands. The country runs from sea
# level to 2107.99 m in this source (Hvannadalshnúkur is 2110 m), so 10 m is
# unambiguous and the whole range carries lines.
#
# NODATA IS -3.4028235e+38, inherited from the source and carried through
# smooth2m untouched. shading.nu deliberately does not relabel it, so the value
# here must match what the tiles actually declare.
#
# THE LOW TAIL LEAVES 111 RINGS IN THE SEA. The source runs -28.9 .. 2107.99 m
# and the smoothed tiles overshoot both ends a little, so gdal_contour emits
# levels at -10, -20 and -30: 111 features of 299899, small closed loops
# offshore where stereo noise over water dips below sea level. They are DELETEd
# after the PostGIS load (see the handoff) rather than cut by a valid_range
# gdal_calc pass, which would spend a full extra read of the country to remove
# 0.04% of the table. The 0 m level is the coastline and is real: 17663 of them.
#
# The high end needs nothing — one ring each at 2090, 2100 and 2110 m, and
# Hvannadalshnukur really is 2110 m, so the overshoot lands on the truth.
#
# GLACIER CONTOURS ARE STALE, NOT BROKEN — AND THAT IS WHY THEY ARE DRAWN.
# ÍslandsDEM mixes ArcticDEM stereo with airborne lidar where it exists, and the
# delivery's own date raster marks the lidar footprint: 8.2% of the grid, dated
# 2008-2013 and 91% of it 2010-2012, which spot checks put over the ice caps and
# the Markarfljót corridor. Measured over Vatnajökull, the elevation step across
# a date boundary is 0.15 m against 0.16 m away from one — 0.9x the local
# roughness, so there is no seam to see. What remains is age: that lidar is
# 12-17 years old and the ice has thinned since, so glacier contours are smooth
# and internally consistent but sit too high. Stale is normal for glaciers on
# any map; discontinuous would not have been, which is why this was measured
# before deciding not to mask `natural=glacier` out.
#
# DATUM: EPSG:3057 is ISN93 / Lambert 1993, a Lambert conic on ETRS89-era
# ground, so the path to 3857 is a plain projected-to-projected transform with
# no datum shift to get wrong — no England/OSTN15 hazard. Note the source is
# also published on ISN2016 (EPSG:8088); do not mix the two.
#
# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/is/is_contours.gpkg \
#          --source-table cont_is_dtm --dest-table contours_is \
#          --source-epsg 3057 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
#      psql "postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        -c 'DELETE FROM contours_is WHERE height_m < 0'    # the 111 sea rings
#
# NOTE THE COLUMN RENAME: the GPKG carries `height`, splitter-rs lands it as
# `height_m`. A DELETE written against `height` fails with "column does not
# exist" and the sea rings stay.
#
# The URL above is the UNIX SOCKET form, which peer-authenticates and needs no
# password. splitter-rs reads neither $PGPASSWORD nor ~/.pgpass, so a TCP URL
# would need the password inline. Pass --simplify-tolerance explicitly; the
# default of 1.0 silently loads a much denser table.
#
# RESTORING ONTO fm5 — TWO TRAPS, BOTH MET IN PRODUCTION:
#
#   1. RUN pg_restore AS `freemap`, NOT AS postgres. `--no-owner` means "give it
#      to the restoring role", not "keep the owner". Restoring as postgres
#      produces a table owned by postgres with no ACL, the renderer connects as
#      `freemap`, and every contour query fails with permission denied — with no
#      error in the journal. `sudo -u freemap` peer-authenticates and gets it
#      right the first time.
#
#   2. PASS THE TABLESPACE. fm5 keeps contours in `contours_ts`, and
#      --no-tablespaces (needed because the dump carries this box's osm_ext,
#      which does not exist there) silently drops the table onto pg_default.
#
#      pg_dump "postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        --format=custom --compress=zstd --no-owner --no-privileges \
#        --table=public.contours_is --file=contours_is.dump
#      scp contours_is.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_is.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_is WHERE ...;
#       curl -sI http://127.0.0.1:4000/<z>/<x>/<y>   # look at X-Attribution
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# If the source tiles change under a re-run, delete the VRT and the
# consolidated raster too — both are skipped on existence.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-is.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/is"
const SRC_DIR  = "/mnt/osm/is/smooth2m"
const EPSG     = "EPSG:3057"

gdal require-proj $EPSG "contours-is.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "is"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/is_dem_10m.vrt"
    dem_tif:      $"($DRIVE)/is/is_dem_10m.tif"
    gpkg:         $"($DRIVE)/is/is_contours.gpkg"
    table:        "cont_is_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-3.4028235e+38"
    epsg:         $EPSG
    interval:     10
    off_interval: 10                         # one pass
    parallel_off: 1
    cachemax_mb:  4096
}
