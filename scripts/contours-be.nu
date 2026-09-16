#!/usr/bin/env nu

# Generate contour lines for Belgium (Wallonia) from the 2 m smoothed DEM tiles
# that shading-be.nu emitted along the way.
# Belgium port of contours-lu.nu / contours-en.nu.
#
# Pipeline: /mnt/osm/be/smooth2m/*.tif   (1250x1250 px windows, 2 m, EPSG:3812)
#             -> one regional VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:3812)
#
# COVERAGE IS WALLONIA ONLY, matching shading-be.nu's INCLUDE_FLANDERS = false.
#   If Flanders is ever switched on, shading-be.nu emits its smooth2m tiles into
#   the same directory and this script picks them up with no change.
#
# THE CONTOURS MUST COME FROM THE SMOOTHED DEM. Contouring raw 1 m LiDAR gives
#   unusable spaghetti — every furrow and forest-floor speckle becomes a closed
#   loop. That is why the shading run bothers to emit smooth2m/ rather than
#   letting this script downsample the source itself. If /mnt/osm/be/smooth2m is
#   empty, run shading-be.nu first; do NOT point this at the province rasters.
#   (Luxembourg measured the difference: 46103 features unsmoothed against 30995
#   smoothed over identical terrain — a third of the lines were noise.)
#
# The consolidation pass is NOT skipped. gdal_contour over a several-thousand-tile
#   VRT is pathologically slow — scattered reads, tiles reopened per scanline —
#   so the tiles are merged into one contiguous raster first.
#
# DATUM: EPSG:3812 IS ETRS89-BASED, SO THERE IS NOTHING TO GET WRONG. projinfo
#   offers exactly one 3812 -> WGS 84 operation and it is a null transform. The
#   older EPSG:31370 (Lambert 72, BD72 datum) offers three candidates at 1-5 m
#   and would be the England/OSTN15 trap. This GPKG is native 3812 regardless.
#
# NODATA. The 2 m tiles carry -9999, written explicitly by shading-be.nu's dem2m
#   step. Belgian terrain runs ~0-694 m (Signal de Botrange) so nothing
#   legitimate approaches the sentinel, but -snodata is passed anyway to keep
#   behaviour identical across countries.
#
# Output: <18TB>/be/belgium_contours.gpkg (layer `cont_be_dtm`, EPSG:3812).
#
# ── HANDOFF TO POSTGIS — THREE THINGS THAT BIT ON LUXEMBOURG ──────────────────
#
# 1. THE SPLITTER DOES NOT CREATE ITS DESTINATION TABLE. It fails with
#    `relation "..." does not exist`. Create it first:
#
#      CREATE TABLE public.cont_be_dtm_split (
#          ogc_fid       serial PRIMARY KEY,
#          id            bigint,
#          height        double precision,
#          wkb_geometry  geometry(LineString, 3857)
#      );
#
#    (splitter-rs inserts into id/height/wkb_geometry and reprojects to 3857.)
#
#    Then run it:
#      DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/be/belgium_contours.gpkg \
#          --source-table cont_be_dtm --dest-table cont_be_dtm_split \
#          --source-epsg 3812 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000
#
#    NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails
#    with "unexpected argument". Omit it for fast Douglas-Peucker.
#
# 2. RESHAPE TO THE SERVING SCHEMA, then rename. The served tables are two
#    columns and no primary key:
#
#      ALTER TABLE public.cont_be_dtm_split DROP COLUMN ogc_fid;
#      ALTER TABLE public.cont_be_dtm_split DROP COLUMN id;
#      ALTER TABLE public.cont_be_dtm_split
#          ALTER COLUMN height TYPE smallint USING height::smallint;
#      ALTER TABLE public.cont_be_dtm_split RENAME COLUMN height TO height_m;
#      ALTER TABLE public.cont_be_dtm_split RENAME TO contours_be;
#      CREATE INDEX contours_be_wkb_geometry_geom_idx
#          ON public.contours_be USING gist (wkb_geometry);
#
#    Index naming matters: en/hr/no/se/lu/sk all use
#    contours_<cc>_wkb_geometry_geom_idx.
#
# 3. pg_restore ONTO fm5 NEEDS --no-tablespaces. The dump carries this box's
#    `osm_ext` tablespace, which does not exist there, and the restore dies with
#    `invalid value for parameter "default_tablespace"`. Also --no-owner, since
#    the `martin` role does not exist on fm5:
#
#      pg_dump "postgresql://martin@localhost/martin" --format=custom \
#        --compress=zstd --no-owner --no-privileges \
#        --table=public.contours_be --file=contours_be.dump
#      scp contours_be.dump fm5:/tmp/
#      # on fm5:
#      pg_restore --dbname=freemap --no-owner --no-privileges --no-tablespaces \
#        --single-transaction /tmp/contours_be.dump
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-be.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/be"
const SRC_DIR  = "/mnt/osm/be/smooth2m"
const EPSG     = "EPSG:3812"

gdal require-proj $EPSG "contours-be.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "be"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/belgium_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/be/belgium_dem_2m.tif"
    gpkg:         $"($DRIVE)/be/belgium_contours.gpkg"
    table:        "cont_be_dtm"                 # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 10
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  16384                            # PER PROCESS: fine for one pass, but raising
                                                   # off_interval runs up to 3 at once = 48 GB of 62.
                                                   # Drop to 2048 when you do.
}
