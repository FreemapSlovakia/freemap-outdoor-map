#!/usr/bin/env nu

# Generate shaded relief for England from the Environment Agency's national 1 m
# LIDAR Composite DTM 2022. England port of shading-no.nu (Norway 1 m).
#
# Source: /media/martin/18TB/en/DTM1 — 5876 GeoTIFF tiles from download-en.nu.
#   Verified uniform on every tile the downloader accepted (it checks each one):
#   5000 x 5000 px, Float32, 1 m, EPSG:27700 embedded, LZW, 128x128 blocks,
#   no overviews, nodata -3.4028235e+38 declared.
#
#   TILES ARE EDGE-TO-EDGE. This is the one structural difference from Norway,
#   whose DTM1 tiles carry a 5 px collar each and overlap by 10 px. Here the
#   5 km grid step matches the 5000 px tile exactly, so a tile has NO context of
#   its own — every pixel of smoothing context has to come from the national VRT.
#   That is already how this script works (windows are cut from the VRT, not from
#   source tiles), so nothing special is needed; but do not "optimise" it into
#   per-tile processing, which would put a visible seam every 5 km.
#
#   NODATA IS THE TRAP. -3.4028235e+38 is Float32's lowest value, and genuine
#   English elevations go BELOW ZERO — a sampled Devon tile bottoms out at
#   -6.06 m of real terrain. So nothing may treat "very negative" as void; only
#   the declared sentinel counts. -vrtnodata -9999 presents one clean sentinel
#   downstream while each source keeps its real -3.4e38 as a per-source <NODATA>
#   (the same mechanism the HR script used to unify four sentinels and the NO
#   script to unify one).
#
# ZOOM=16, measured rather than inherited. sample-zoom-en.nu rendered NY2005 —
#   Scafell Pike, 134-978 m, the roughest ground in England, i.e. the best case
#   for a finer zoom — at z15/z16/z17 through this exact pipeline, then upscaled
#   each to z17's grid (what a client does when it overzooms) and differenced
#   over 13.5 M pixels:
#
#     z15 -> z17   mean 6.12/255   p95 23.0   31.2% of pixels off by >5
#     z16 -> z17   mean 1.32/255   p95  4.0    3.2% of pixels off by >5
#
#   z16 is indistinguishable; z15 is visibly lossy. Norway measured 1.07 and 2.4%
#   on its own best case and chose z16 too. The ceiling is the source, not the
#   grid: the DTM is a 1 m grid and --filter 11 strips most sub-11 m variation
#   before the hillshade is computed, so z17 resamples detail already removed —
#   for 137 GB against 44 GB and roughly 4x the render time.
#
#   MIND THE UNITS. A z16 pixel here is 1.34-1.54 GROUND metres, not the 2.39 m
#   that -tr says: Mercator pixels shrink as cos(lat), so at 50-56 degN the raw
#   -tr overstates resolution by about a third. z16 therefore sits slightly
#   coarser than the 1 m source, which the measurement says does not matter.
#   Re-run sample-zoom-en.nu on a different TILE to test that claim elsewhere.
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
# WHY NO gdal_retile (England followed Norway; every country now uses the shared
# window grid in lib/shading.nu):
#
#   1. DISK. retiled/ + smooth/ for all of England at 1 m is ~1 TB against ~400 GB
#      free on the NVMe. So windows are cut from the national VRT on demand,
#      processed, and their intermediates deleted immediately — only tiles/
#      accumulates. Peak scratch is PARALLEL x one window, not the whole country.
#
#   2. EMPTINESS. England's BNG bbox is ~383,000 km2 for ~130,000 km2 of land.
#      Deriving windows from the real tile footprints (each 5 km tile = 2x2
#      windows of 2.5 km) gives windows that contain data by construction.
#
# WINDOW ORIGINS ARE READ FROM THE TILES, NOT DECODED FROM THEIR NAMES. An OS
#   grid reference like NY2005 is decodable in principle, but getting the
#   500 km/100 km letter pair wrong would silently shift a whole region. One
#   gdalinfo pass over the tiles, cached to a TSV, is authoritative and costs a
#   minute. Delete the TSV to rebuild.
#
# CONTOURS. smooth/ cannot be kept (~520 GB), so each window also emits a 2 m
#   downsample of its smoothed DEM into smooth2m/, which contours-en.nu contours
#   directly. `average` resampling is nodata-aware; bilinear/cubic would blend
#   nodata into its neighbours.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING, not a style choice.
#   feature-preserving-smoothing does I/O via the `wbgeotiff` crate, which does
#   not parse the TIFF Predictor tag (317) — fed PREDICTOR=2/3 float data it
#   decodes byte-shuffled deltas as garbage and emits +/-Inf rasters that render
#   as static, WITHOUT erroring. (Documented in shading-it.nu, verified 2026-07-17.)
#
# Smoothing is 11/16/6/6, the Poland/Croatia/Norway 1 m settings — the filter is
#   a pixel count, so 11 px = 11 m on 1 m data.
#
# Resumable at window granularity: a window whose tiles/<id>.tif exists is
# skipped, and an all-nodata window leaves a tiles/<id>.empty marker so it is not
# re-cut on the next run. A window that throws is recorded in failed/ and skipped
# so one bad source tile cannot take down a multi-day run. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/shading-en.nu

# WHERE THE I/O GOES, and why.
#
#   scratch      /dev/shm  — every per-window intermediate (the window cut, the
#                 smoothed DEM, three hillshades, three warps, RGBA) is written
#                 to tmpfs and deleted on the way out. 24 windows in flight peak
#                 at a few GB of the 32 GB available, so the hot path is RAM and
#                 never touches a disk at all.
#   source reads /media/martin/18TB/en/DTM1 — 330 GB, has to stay on the HDD;
#                 read once, largely sequentially, so the HDD costs little here.
#   tiles/       /mnt/osm/en — NVMe, ON PURPOSE. This is 23504 small files that
#                 are written once and then read back in full by the merge. The
#                 18TB is ntfs-3g (FUSE, since this kernel has no ntfs3), where
#                 every small-file create is a userspace round-trip — by far the
#                 worst I/O in the run, on the worst filesystem for it.
#                 Measured cost: the z16 zoom sample is 1.09 MB/km2, so ~141 GB
#                 for England, and that is an upper bound because the sample is
#                 Scafell Pike and rough terrain compresses worst. With
#                 smooth2m/ at ~130 GB that leaves ~127 GB free on the NVMe.
#                 (An earlier comment here said tiles/ was "too big for the
#                 NVMe" — that was a guess made before the sample existed.)
#                 DELETE tiles/ once shading.tif is built; it is pure
#                 intermediate and frees ~141 GB.
#   shading.tif  stays on the 18TB: one big sequential write, and the NVMe
#                 headroom is worth more to tiles/.
#
# THE 2 m CONTOUR DEM NOW COMES FROM THE FILLED RASTER. It used to be emitted
#   from the unfilled $smooth, before gdal_fillnodata ran — so gdal_contour broke
#   a line at every void while the hillshade beside it looked continuous, because
#   the hillshade was the filled one. The shared pipeline owns that step order
#   now. Any smooth2m/ tile written before this change is unfilled; delete them
#   (not tiles/) to re-emit. tiles/ is unaffected.
#
use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_DIR   = "/media/martin/18TB/en/DTM1"
const DATA_DIR  = "/mnt/osm/en"                  # VRT, origin cache, smooth2m/ on NVMe
const EPSG      = "EPSG:27700"                   # OSGB36 / British National Grid
const NODATA    = "-9999"                        # unified nodata presented by en.vrt
const PARALLEL  = 24

const TILE_M    = 5000                           # source tile size, m (= px)
const STEP      = 2500                           # window size, m; 2x2 per source tile

let VRT     = $"($DATA_DIR)/en.vrt"
let ORIGINS = $"($DATA_DIR)/tile_origins.tsv"    # tile<TAB>xmin<TAB>ymax, cached

gdal assert-mounted "/media/martin/18TB"
gdal require-proj $EPSG "shading-en.nu"

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run download-en.nu first"}
}

mkdir $DATA_DIR

# ── 0. National VRT, one unified nodata ───────────────────────────────────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing \(delete to force a rebuild\)"
} else {
    print $"==> Building national VRT ($VRT) — unified nodata ($NODATA)"
    let tiles = (glob $"($SRC_DIR)/*.tif")
    print $"  ($tiles | length) tiles"
    gdal build-vrt $tiles $VRT --extra [-vrtnodata $NODATA] --index $"($DATA_DIR)/_idx_en"
}

# ── 1. Tile origins, one gdalinfo pass, cached ────────────────────────────────
# The window ids are anchored to each SOURCE tile, not to a national grid, so
# adding a tile cannot renumber its neighbours.

if not ($ORIGINS | path exists) {
    print "==> Reading tile origins (one gdalinfo pass, cached)"
    let rows = (
        glob $"($SRC_DIR)/*.tif"
          | par-each -t $PARALLEL {|f|
              let g = (gdalinfo -json $f | from json | get geoTransform)
              {tile: ($f | path parse | get stem), xmin: $g.0, ymax: $g.3}
            }
          | sort-by tile
    )
    $rows | to tsv | save -f $"($ORIGINS).tmp"
    mv $"($ORIGINS).tmp" $ORIGINS
    print $"  ($rows | length) origins"
}

let origins = (
    open --raw $ORIGINS | from tsv
      | each {|t| {id: $t.tile, x0: ($t.xmin | into float), y1: ($t.ymax | into float)} }
)

shading run {
    code:      "en"
    src:       $VRT
    data_root: $DATA_DIR
    tiles_dir: $"($DATA_DIR)/tiles"               # NVMe ON PURPOSE — see header
    out_tif:   "/media/martin/18TB/en/shading.tif"
    nodata:    $NODATA
    zoom:      16                                # MEASURED — see header
    parallel:  $PARALLEL
    tmpdir:    "/dev/shm"
    step:      $STEP
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   5                                 # px; 5 m at 1 m
    dem_tr:    2                                 # m; what contours-en.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "tiles", origins: $origins, nx: ($TILE_M // $STEP), ny: ($TILE_M // $STEP)}
}
