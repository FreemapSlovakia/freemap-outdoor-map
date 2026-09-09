#!/usr/bin/env nu

# Generate contour lines for all of England from the 2 m smoothed DEM tiles that
# shading-en.nu emitted along the way.
# England port of contours-no.nu (Norway 2 m) / contours-hr.nu (Croatia 1 m).
#
# Pipeline: /mnt/osm/en/smooth2m/*.tif   (2500x2500 px windows, 2 m, EPSG:27700)
#             -> one national VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:27700)
#
# As in Norway, and unlike Croatia, there is NO per-tile cropping and NO 1 m -> 2 m
# downsampling to do here: shading-en.nu's dem2m step already wrote each window
# cropped to its exact 2.5 km extent (-projwin at the window bounds, collar
# excluded) and already at 2 m, produced with nodata-aware `average` while the
# smoothed DEM was still in RAM. So these tiles tile the plane seamlessly with no
# overlap and no gaps, and gdalbuildvrt is enough.
#
# The consolidation pass is NOT skipped. gdal_contour over a many-thousand-tile
# VRT is pathologically slow — scattered reads, tiles reopened per scanline — so
# the tiles still have to be merged into one contiguous raster first. What the
# 2 m handoff saves is the expensive half of Croatia's consolidation (8.5 h
# there); this pass now only copies 2 m data instead of reading 1 m and
# resampling it.
#
# Consolidated DEM goes on the 18TB, not the NVMe: England's BNG bbox at 2 m is
# ~96e9 cells, a few hundred GB. gdal_contour reads it sequentially, so spinning
# rust costs little.
#
# NODATA. The 2 m tiles carry -9999, written explicitly by shading-en.nu's
# dem2m step, NOT the -3.4028235e+38 of the delivered source. That matters here:
# genuine English elevations go below zero (real terrain down to about -6 m in
# the Fens and on reclaimed coast), and 0.00 m coastal contours are legitimate.
# Because out-of-coverage is -9999 and never 0, those low and zero contours are
# preserved rather than being mistaken for void.
#
# DATUM: THE DELIVERED PRODUCTS ARE ~1.9 m OFF TRUE WGS84. KNOWN, ACCEPTED.
#
# EPSG:27700 is on the OSGB36 datum, and getting from there to WGS84/Web
# Mercator needs a datum shift. The correct one is OSTN15, OS's official NTv2
# grid (~0.1 m). It was NOT installed when these products were built, and PROJ
# does not error in that case — it silently falls back to a 7-parameter Helmert
# ("OSGB36 to WGS 84 (6)", 2 m stated accuracy).
#
# Measured 2026-08-17 against the real grid, 238 samples across England:
#
#     median 1.90 m   p95 3.57 m   max 4.67 m
#     mid-England 1.89 m | Lake District 0.94 m | London 1.74 m
#
# A z16 pixel here is 1.34-1.54 ground metres, so this is 1-3 px of systematic
# misregistration against OSM, and it VARIES SPATIALLY — no constant offset can
# correct it. Both shading.tif and contours_en carry it, so at least they agree
# with each other. england_contours.gpkg and the raw DTM tiles are native
# EPSG:27700 and therefore unaffected; only reprojected outputs are.
#
# MIXED-DATUM HAZARD — READ BEFORE ANY PARTIAL RE-RENDER. The OSTN15 grid is now
# present at ~/.local/share/proj/uk_os_OSTN15_NTv2_OSGBtoETRS.tif, which the geo
# env's PROJ picks up automatically. So re-rendering a handful of windows into
# the EXISTING tiles/ would place them ~1.9 m from their neighbours and leave a
# visible seam. If you re-render, re-render EVERYTHING: delete tiles/ and rebuild
# the mosaic from scratch. Do not resume a partial run across this change.
#
# OSTN15 IS NOW INSTALLED BOX-WIDE (/usr/share/proj/, 2026-08-17), verified to
# match pyproj digit-for-digit from PostGIS. So the whole machine is on OSTN15
# while these two products are not — the mixed-datum hazard above now applies to
# BOTH: re-running the splitter alone would put contours ~1.9 m from the shading,
# i.e. they would stop agreeing with each other. Currently they are consistently
# wrong together, which is the better of the two bad states.
#
# TO FIX PROPERLY (a full rebuild, ~17 h shading + ~2 h splitter): no setup left
#   — delete tiles/ and shading.tif, re-run shading-en.nu, then re-run the
#   splitter from the (unaffected) EPSG:27700 GPKG. Both pick up OSTN15 on their
#   own.
#
# WHEN COMPUTING A 4326 BBOX AFTER THAT: OSTN15 covers only the GB landmass, not
# the whole BNG rectangle, so densifying along the rectangle edges returns inf
# outside its coverage. Clamp to the grid extent, or densify over the data
# footprint rather than the raster rectangle.
#
# Output: /media/martin/18TB/en/england_contours.gpkg (layer `cont_en_dtm`,
# EPSG:27700).
# Handoff to the splitter (split <=1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /media/martin/18TB/en/england_contours.gpkg \
#       --source-table cont_en_dtm --dest-table cont_en_dtm_split \
#       --source-epsg 27700 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails with
# "unexpected argument". Omit it for fast Douglas-Peucker; pass it bare for Visvalingam.
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-en.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/en"
const SRC_DIR    = "/mnt/osm/en/smooth2m"                        # 2 m tiles from shading-en.nu
const INTERVAL   = 10                                            # contour interval, metres
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_en_dtm"                                 # layer name inside the GPKG
const VRT        = "england_dem_2m.vrt"
const DEM_TIF    = "/media/martin/18TB/en/england_dem_2m.tif"    # consolidated DEM (EPSG:27700)
const GPKG       = "/media/martin/18TB/en/england_contours.gpkg" # splitter input (EPSG:27700)


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
    error make {msg: $"($SRC_DIR) is empty — run shading-en.nu first"}
}

# ── 1. National VRT straight from the 2 m tiles (no cropping needed) ──────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing"
} else {
    print "==> Building national VRT from the 2 m tiles"
    let idx = "_idx_en_cont"
    glob $"($SRC_DIR)/*.tif" | save -f $idx
    print $"  (open $idx | lines | length) tiles"
    gdalbuildvrt -vrtnodata $NODATA -input_file_list $idx $"($VRT).tmp" o> /dev/null
    rm $idx
    mv $"($VRT).tmp" $VRT
}

# ── 2. Consolidate the VRT into ONE contiguous raster ─────────────────────────
# No reprojection and no resampling — the tiles are already EPSG:27700 at 2 m.
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

# ── 3. gdal_contour on the consolidated raster -> GPKG (EPSG:27700) ───────────
# Single-threaded but reads sequentially. Resumable: delete the GPKG.

if ($GPKG | path exists) {
    print $"==> ($GPKG) already exists — delete it to re-generate; skipping"
} else {
    print $"==> Generating contours from ($DEM_TIF) -> ($GPKG) — this will take hours"
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
    print $"==> Done -> ($GPKG). Next: run splitter-rs \(--source-epsg 27700\); see header."
}
