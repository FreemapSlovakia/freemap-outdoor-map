#!/usr/bin/env nu

# Generate contour lines for Bayern from the 2 m smoothed DEM tiles that
# shading-de-by.nu emitted along the way. Port of contours-be.nu.
#
# Pipeline: /mnt/osm/de-by/smooth2m/*.tif  (1250x1250 px windows, 2 m, EPSG:25832)
#             -> one state VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:25832)
#
# THE CONTOURS MUST COME FROM THE SMOOTHED DEM. Contouring raw 1 m LiDAR gives
#   unusable spaghetti — every furrow and forest-floor speckle becomes a closed
#   loop. That is why the shading run bothers to emit smooth2m/ rather than
#   letting this script downsample the source itself. If smooth2m is empty, run
#   shading-de-by.nu first; do NOT point this at the DGM1 tiles as a shortcut.
#   (Luxembourg measured the difference: 46103 features unsmoothed against 30995
#   smoothed over identical terrain — a third of the lines were noise.)
#
# The consolidation pass is NOT skipped. gdal_contour over a many-thousand-tile
#   VRT is pathologically slow — scattered reads, tiles reopened per scanline —
#   so the tiles are merged into one contiguous raster first.
#
# NO DATUM HAZARD. EPSG:25832 is ETRS89 / UTM 32N, so the path to WGS 84 is a
#   null transform — nothing to install, nothing for PROJ to get silently wrong.
#   Contrast England (OSGB36, needs OSTN15) and Flanders (BD72, needs the IGN
#   NTv2 grid). This GPKG is native 25832 regardless.
#
# EDGE TRIANGULATION AT THE STATE BORDER. Measured 2026-09-07 near 50.209 N,
#   10.723 E: surface roughness falls from 0.0273 m in the interior to 0.0142 m
#   in the outer 50 m of coverage — the Bavarian DGM1 is TIN-interpolated where
#   the LiDAR ends. Contours inherit that: expect slightly too-smooth, slightly
#   too-straight lines within ~300 m of the state boundary. A cutline at the
#   administrative border would remove it; not applied yet.
#
# ELEVATION RANGE. Bayern spans roughly 100 m (Lower Main valley) to 2962 m
#   (Zugspitze), so at a 10 m interval expect ~290 distinct levels — far more
#   than the flat countries, and a correspondingly large feature count.
#
# Output: <18TB>/de-by/bayern_contours.gpkg (layer `cont_de_by_dtm`, EPSG:25832).
#
# ── HANDOFF TO POSTGIS — THE THINGS THAT BIT ON EARLIER COUNTRIES ─────────────
#
# 1. LOAD IT. The splitter now creates the destination table in its final
#    serving shape (height_m smallint, wkb_geometry geometry(LineString,3857))
#    and builds the GiST index itself when the load finishes, so the old dance
#    — hand-CREATE a wide table, then DROP ogc_fid, DROP id, cast height to
#    smallint, rename the column, rename the table, CREATE INDEX — is gone.
#
#      DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_by/bayern_contours.gpkg \
#          --source-table cont_de_by_dtm --dest-table contours_de_by \
#          --source-epsg 25832 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
#    THE SPLITTER NEEDS THE PASSWORD IN THE URL. It uses rust-postgres, which
#    — unlike libpq — reads neither $PGPASSWORD nor ~/.pgpass, and fails with
#    "password missing" if the URL has none. So interpolate $PGPASSWORD into
#    the URL as above: the secret stays out of this public repo, and passing it
#    as an environment variable rather than --database-url also keeps it out of
#    `ps`. (psql, pg_dump and GDAL's PG driver do read $PGPASSWORD, so they
#    need no password in their connection strings at all.)
#
#    NOTE: `--simplify-high-quality` and `--drop-existing` are boolean FLAGS —
#    passing either a value fails with "unexpected argument".
#
#    PASS --simplify-tolerance 2 EXPLICITLY. The default is 1.0, and leaving it
#    out silently loads a denser table than en/lu/be/hr, which all used 2 (the
#    first Bayern load did exactly that: 1066570 rows / 1130 MB at the default).
#    The tolerance is applied in the SOURCE CRS, i.e. metres here.
#
#    smallint holds Bayern's range comfortably (max 2962 m).
#    Index naming: en/hr/no/se/lu/sk/be all use contours_<cc>_wkb_geometry_geom_idx.
#
# 2. pg_restore ONTO fm5 NEEDS --no-tablespaces (the dump carries this box's
#    `osm_ext`, which does not exist there) AND --no-owner (no `martin` role):
#
#      pg_restore --dbname=freemap --no-owner --no-privileges --no-tablespaces \
#        --single-transaction /tmp/contours_de_by.dump
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-by.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_by"
const SRC_DIR  = "/mnt/osm/de_by/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-de-by.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "deby"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/bayern_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_by/bayern_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_by/bayern_contours.gpkg"
    table:        "cont_de_by_dtm"                 # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 100
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
