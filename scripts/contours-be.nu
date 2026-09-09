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

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/be"
const SRC_DIR    = "/mnt/osm/be/smooth2m"          # 2 m tiles from shading-be.nu
const INTERVAL   = 10                              # contour interval, metres
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_be_dtm"                   # layer name inside the GPKG
const VRT        = "belgium_dem_2m.vrt"

# ── Drive discovery ───────────────────────────────────────────────────────────
# udisks2 moves the removable 18TB between /media and /run/media, and the unused
# path survives as an empty root-owned directory on / — so a stale hardcoded
# path does not fail, it silently fills the root filesystem.
def find-drive []: nothing -> string {
    let found = (
        ["/run/media/martin/18TB" "/media/martin/18TB"]
          | where {|p| (do { mountpoint -q $p } | complete).exit_code == 0 }
    )
    if ($found | is-empty) {
        error make {msg: "the 18TB drive is not mounted at /media/martin/18TB or /run/media/martin/18TB. Check `lsblk -o NAME,LABEL,SIZE,MOUNTPOINT` (label 18TB)."}
    }
    $found | first
}

let DRIVE   = (find-drive)
let DEM_TIF = $"($DRIVE)/be/belgium_dem_2m.tif"      # consolidated DEM (EPSG:3812)
let GPKG    = $"($DRIVE)/be/belgium_contours.gpkg"   # splitter input (EPSG:3812)

print $"==> drive: ($DRIVE)"

# PROJ sanity — a missing PROJ database degrades every CRS to ENGCRS instead of
# failing loudly. Always run inside the geo env, never with its bin on PATH.
let _probe = (do { gdalsrsinfo -o proj4 "EPSG:3812" } | complete)
if $_probe.exit_code != 0 or ($_probe.stdout | str trim | is-empty) {
    error make {msg: "PROJ cannot resolve EPSG:3812 — run via: nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu contours-be.nu"}
}

cd $DATA_DIR

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run shading-be.nu first \(it emits the smoothed 2 m tiles\)"}
}

# ── 1. Regional VRT straight from the 2 m tiles (no cropping needed) ──────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing"
} else {
    print "==> Building regional VRT from the 2 m tiles"
    let idx = "_idx_be_cont"
    glob $"($SRC_DIR)/*.tif" | save -f $idx
    print $"  (open $idx | lines | length) tiles"
    gdalbuildvrt -vrtnodata $NODATA -input_file_list $idx $"($VRT).tmp" o> /dev/null
    rm $idx
    mv $"($VRT).tmp" $VRT
}

# ── 2. Consolidate the VRT into ONE contiguous raster ─────────────────────────
# No reprojection and no resampling — the tiles are already EPSG:3812 at 2 m.

if ($DEM_TIF | path exists) {
    print $"==> ($DEM_TIF) exists — reusing"
} else {
    print $"==> Consolidating DEM -> ($DEM_TIF) — one full pass"
    let tmp = $"($DEM_TIF).tmp"
    rm -f $tmp
    (gdal_translate
      --config GDAL_CACHEMAX 16384
      -of GTiff
      -a_nodata $NODATA
      -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES
      -co NUM_THREADS=ALL_CPUS -co BIGTIFF=YES
      $VRT $tmp)
    mv $tmp $DEM_TIF
}

# ── 3. gdal_contour on the consolidated raster -> GPKG (EPSG:3812) ────────────

if ($GPKG | path exists) {
    print $"==> ($GPKG) already exists — delete it to re-generate; skipping"
} else {
    print $"==> Generating contours from ($DEM_TIF) -> ($GPKG)"
    let tmp = $"($GPKG).tmp"
    rm -f $tmp
    (gdal_contour
      --config GDAL_CACHEMAX 16384
      -f GPKG
      -nln $TABLE
      -i $INTERVAL
      -a $HEIGHT_COL
      -snodata $NODATA
      -lco SPATIAL_INDEX=NO
      $DEM_TIF $tmp)
    mv $tmp $GPKG
    print $"==> Done -> ($GPKG). Next: create the dest table, then splitter-rs \(--source-epsg 3812\); see header."
}
