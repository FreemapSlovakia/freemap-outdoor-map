#!/usr/bin/env nu

# Generate contour lines for Schleswig-Holstein from the 2 m smoothed DEM tiles
# that shading-de-sh.nu emitted along the way.
# Port of contours-de-bw.nu. Output: <18TB>/de_sh/schleswig_holstein_contours.gpkg,
# EPSG:25832.

# Must come from the smoothed DEM: contouring raw 1 m lidar gives spaghetti,
# every furrow and forest-floor speckle a closed loop. Luxembourg measured 46103
# features unsmoothed against 30995 smoothed over identical terrain. If
# /mnt/osm/de_sh/smooth2m is empty, run shading-de-sh.nu first.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# INTERVAL 10 m, as everywhere except the Netherlands. The state runs from
# about -3.7 m in the Wilstermarsch — the lowest land in Germany — to 168 m on
# the Bungsberg, so the whole state fits in eighteen levels.

# EXPECT A DENSE 0 m LINE AND DO NOT READ IT AS DAMAGE. A fifth of
# Schleswig-Holstein is marsh within a metre or two of sea level, so the zero
# contour traces every polder edge and drainage ditch. Niedersachsen, the other
# marsh state, carries 741866 features at 0 m out of 1594592 — 47% of its whole
# table — and is deployed that way.

# MOST OF THE STATE WILL CARRY FEW LINES. Roughness p50 is 0.033 over 18557
# rasters and the sample windows run 1.0-2.6° median slope. Do not reach for a
# 5 m interval: the renderer's height filter draws multiples of 10 below z15,
# so the extra lines would roughly double the table while showing nothing at
# the zooms where flat country is actually viewed.

# DATUM: EPSG:25832 is ETRS89 / UTM 33N, so the path to 3857 is a null transform
# and there is nothing to get wrong — no England/OSTN15 hazard.

# NODATA IS -9999 AND MEANS "NO TILE", NOT "NO DATA HERE". The delivery
# declares no nodata at all and 0.00 is real marsh, so all.vrt sets -vrtnodata
# -9999 purely to fill ground no tile covers; -9999 appears nowhere in the
# sources. shading-de-sh.nu's smoothed tiles inherit it, so there is nothing to
# undo here.
#
# 0.00 IS REAL AND MUST SURVIVE. 4426 rasters carry under 1% zeros scattered
# through ground running about -1.6 m to +3 m, which is marsh crossing the zero
# level; only 37 rasters are more than half zero and those are Baltic. A
# -srcnodata 0 anywhere in this pipeline would delete the coast.
#
# EXPECT CONTOURS DOWN TO -10 m. 1150 rasters hold 97 million pixels below -2 m
# and the floor is about -3.7 m at Neuendorf-Sachsenbande, so a -10 m line is
# the last one and a 0 m line is ordinary ground, not a coastline artefact.
#
# LAKES AND THE BALTIC ARE FLAT SURFACES, not holes: inland water carries its
# real surface height, so lidar's failure to penetrate leaves an interpolated
# plane that yields no contours across its width. That is right, and the
# renderer draws water over the top anyway.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_sh/schleswig_holstein_contours.gpkg \
#          --source-table cont_de_sh_dtm --dest-table contours_de_sh \
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
#        --table=public.contours_de_sh --file=contours_de_sh.dump
#      scp contours_de_sh.dump fm5:/tmp/
#      # on fm5:
#      sudo -u freemap env PGOPTIONS='-c default_tablespace=contours_ts' \
#        pg_restore --dbname=freemap --no-owner --no-privileges \
#        --no-tablespaces --single-transaction /tmp/contours_de_sh.dump
#
#   Verify before declaring it live — row counts prove nothing about whether the
#   renderer can read it:
#       SET ROLE freemap; SELECT count(*) FROM contours_de_sh WHERE ...;
#       curl http://127.0.0.1:4000/<z>/<x>/<y>

# Resumable: the VRT, consolidated raster and GPKG are each skipped if present.
#
# THE OFFSET PARTIALS ARE SKIPPED ON EXISTENCE ALONE. If the source tiles change
# under a re-run, delete schleswig_holstein_dem_2m.vrt, schleswig_holstein_dem_2m.tif AND
# schleswig_holstein_contours_off*.gpkg — otherwise the contour stage prints "skip
# offset N — already done" and republishes the previous run's vectors from a
# stale partial. This cost a full wasted cycle on the Netherlands; the giveaway
# was an output byte-identical in size to the old one.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-sh.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_sh"
const SRC_DIR  = "/mnt/osm/de_sh/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-de-sh.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "desh"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/schleswig_holstein_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_sh/schleswig_holstein_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_sh/schleswig_holstein_contours.gpkg"
    table:        "cont_de_sh_dtm"              # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
