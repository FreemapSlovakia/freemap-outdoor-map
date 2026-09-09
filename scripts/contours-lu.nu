#!/usr/bin/env nu

# Generate contour lines for all of Luxembourg from the 2 m smoothed DEM tiles
# that shading-lu.nu emitted along the way.
# Luxembourg port of contours-en.nu (England 2 m) / contours-no.nu (Norway 2 m).
#
# Pipeline: /mnt/osm/lu/smooth2m/*.tif   (1250x1250 px windows, 2 m, EPSG:2169)
#             -> one national VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:2169)
#
# As in England and Norway, and unlike Croatia, there is NO per-tile cropping and
# NO 1 m -> 2 m downsampling to do here: shading-lu.nu's dem2m step already wrote
# each window cropped to its exact 2.5 km extent (-projwin at the window bounds,
# collar excluded) and already at 2 m, produced with nodata-aware `average` while
# the smoothed DEM was still in RAM. So these tiles tile the plane seamlessly with
# no overlap and no gaps, and gdalbuildvrt is enough.
#
# THE CONTOURS MUST COME FROM THE SMOOTHED DEM. Contouring raw 1 m LiDAR produces
# unusable spaghetti — every furrow and forest-floor speckle becomes a closed
# loop. That is the entire reason the shading run bothers to emit smooth2m/
# rather than letting this script downsample the national raster itself. If
# /mnt/osm/lu/smooth2m is empty, run shading-lu.nu first; do NOT point this script
# at luxembourg_dem_1m.tif as a shortcut.
#
# The consolidation pass is NOT skipped. gdal_contour over a many-hundred-tile VRT
# is pathologically slow — scattered reads, tiles reopened per scanline — so the
# tiles still have to be merged into one contiguous raster first. Luxembourg is
# small enough that this is minutes, not the 8.5 h it cost Croatia.
#
# DATUM: NO GRID SHIFT EXISTS, AND NONE IS NEEDED. projinfo offers exactly two
#   EPSG:2169 -> EPSG:4326 operations, LUREF to WGS 84 (4) (Molodensky-Badekas)
#   and (3) (7-parameter Helmert), both stated at 1 m, NEITHER referencing an
#   NTv2 grid. There is no Luxembourg OSTN15 to install and therefore none of
#   England's mixed-datum hazard: nothing can silently change under a re-run.
#   This GPKG is native EPSG:2169 and unaffected regardless.
#
# NODATA. The 2 m tiles carry -9999, written explicitly by shading-lu.nu's dem2m
# step. Luxembourg's terrain runs ~130-560 m so nothing legitimate approaches the
# sentinel, but -snodata is passed anyway to keep the behaviour identical to the
# other countries.
#
# Output: /media/martin/18TB/lu/luxembourg_contours.gpkg (layer `cont_lu_dtm`,
# EPSG:2169).
# Handoff to the splitter (split <=1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /media/martin/18TB/lu/luxembourg_contours.gpkg \
#       --source-table cont_lu_dtm --dest-table cont_lu_dtm_split \
#       --source-epsg 2169 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails with
# "unexpected argument". Omit it for fast Douglas-Peucker; pass it bare for Visvalingam.
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-lu.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/lu"
const SRC_DIR    = "/mnt/osm/lu/smooth2m"                            # 2 m tiles from shading-lu.nu
const INTERVAL   = 10                                                # contour interval, metres
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_lu_dtm"                                     # layer name inside the GPKG
const VRT        = "luxembourg_dem_2m.vrt"
const DEM_TIF    = "/media/martin/18TB/lu/luxembourg_dem_2m.tif"     # consolidated DEM (EPSG:2169)
const GPKG       = "/media/martin/18TB/lu/luxembourg_contours.gpkg"  # splitter input (EPSG:2169)


# ── Mount guard ───────────────────────────────────────────────────────────────
# The 18TB is removable and udisks2 has already moved it once (/media/... ->
# /run/media/... across the 2026-08-16 reboot, with the device letter shifting
# too). The old path survives as an empty root-owned directory on /, which has
# far less free space than this script needs — so a stale path does not fail, it
# silently fills the root filesystem. Fail fast instead.
def assert-mounted [p: string]: nothing -> nothing {
    let res = (do { mountpoint -q $p } | complete)
    if $res.exit_code != 0 {
        error make {msg: $"($p) is not a mountpoint — the 18TB drive is not mounted there. Check `lsblk -o NAME,LABEL,SIZE,MOUNTPOINT` \(label 18TB\) and repoint the paths in this script."}
    }
}

assert-mounted "/media/martin/18TB"

cd $DATA_DIR

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run shading-lu.nu first \(it emits the smoothed 2 m tiles\)"}
}

# ── 1. National VRT straight from the 2 m tiles (no cropping needed) ──────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing"
} else {
    print "==> Building national VRT from the 2 m tiles"
    let idx = "_idx_lu_cont"
    glob $"($SRC_DIR)/*.tif" | save -f $idx
    print $"  (open $idx | lines | length) tiles"
    gdalbuildvrt -vrtnodata $NODATA -input_file_list $idx $"($VRT).tmp" o> /dev/null
    rm $idx
    mv $"($VRT).tmp" $VRT
}

# ── 2. Consolidate the VRT into ONE contiguous raster ─────────────────────────
# No reprojection and no resampling — the tiles are already EPSG:2169 at 2 m.
# This exists purely so gdal_contour can read sequentially instead of reopening
# tiles per scanline.

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

# ── 3. gdal_contour on the consolidated raster -> GPKG (EPSG:2169) ────────────
# Single-threaded but reads sequentially. Resumable: delete the GPKG.

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
    print $"==> Done -> ($GPKG). Next: run splitter-rs \(--source-epsg 2169\); see header."
}
