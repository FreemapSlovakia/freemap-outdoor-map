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
# As in every country now, there is NO per-tile cropping and NO 1 m -> 2 m
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

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/en"
const SRC_DIR  = "/mnt/osm/en/smooth2m"
const EPSG     = "EPSG:27700"

gdal require-proj $EPSG "contours-en.nu"

gdal assert-mounted "/media/martin/18TB"

contours run {
    code:         "en"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/england_dem_2m.vrt"
    dem_tif:      "/media/martin/18TB/en/england_dem_2m.tif"
    gpkg:         "/media/martin/18TB/en/england_contours.gpkg"
    table:        "cont_en_dtm"                 # layer name inside the GPKG
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
