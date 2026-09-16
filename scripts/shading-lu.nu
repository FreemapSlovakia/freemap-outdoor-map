#!/usr/bin/env nu

# Generate shaded relief for Luxembourg from the Administration du cadastre et de
# la topographie's LiDAR 2024 MNT. Luxembourg port of shading-en.nu (England 1 m).
#
# Source: ONE national Cloud-Optimized GeoTIFF, not a tile directory. This is the
#   structural difference from every previous country and it removes two whole
#   stages: there is no downloader, no per-tile verification, no national VRT and
#   no tile_origins.tsv. Step 0 below fetches the raster in a single command and
#   the window grid is derived from its own extent.
#
#   https://data.public.lu/en/datasets/lidar-2019-modele-numerique-de-terrain-mnt/
#   (the 2024 campaign is published under the same dataset family; see LICENCE)
#
#   Survey flown 2024 at >30 points/m2, vertical precision <10 cm (sigma 5.5 cm) —
#   double the 2019 density. Next acquisition planned winter 2027-2028, so this is
#   current for years. MNT = bare terrain; MNS is the surface model, do not use it.
#
# LICENCE: CC0. No attribution obligation at all — the only country here with no
#   credit line to carry. (Contrast England's mandatory verbatim Environment
#   Agency wording.) Crediting ACT anyway is good manners, not a condition.
#
# WE TAKE THE 1 m OVERVIEW, NOT THE 0.5 m FULL RESOLUTION. The delivered COG is
#   0.5 m / 114617 x 163687 / Float64 / 38.3 GB. Its first overview level is
#   exactly half — 57308 x 81843 at 1.000 m — so `-oo OVERVIEW_LEVEL=0` reads the
#   1 m grid directly over HTTP range requests and never transfers the 0.5 m data
#   at all. 4.8 GB fetched in 11 min against 38.3 GB.
#
#   Float64 -> Float32 on the way in: elevation here spans ~130-560 m, where
#   Float32 resolves ~3e-5 m. Halves the volume, loses nothing.
#
#   Why not 0.5 m: --filter 11 is a PIXEL count, so on 0.5 m data it would smooth
#   over 5.5 m instead of 11 m, i.e. a different (weaker) filter for the same
#   nominal setting — and 4x the pixels to render z17 that is already finer than
#   the smoothed signal. 1 m keeps the filter comparable with PL/HR/NO/EN.
#
# ZOOM=17, chosen visually, against the measurement. Measured first, on smoothed
#   data through this exact pipeline (Mullerthal, the roughest ground here):
#
#     z15 -> z17   mean 1.81/255   p95 5.0    4.83% of pixels off by >5
#     z16 -> z17   mean 1.03/255   p95 3.0    2.57% of pixels off by >5
#
#   2.57% is England's z16 figure (3.2%) — by the England rule that says z16.
#   BUT the samples were then looked at, and z17 read as visibly better; it also
#   matches Slovakia, the other 1 m country (sk/final.tif is 1.1943285 m/px).
#   The cost argument that decided England does not apply at this size: 2.5 GB
#   against 632 MB, versus England's 137 GB against 44 GB.
#
#   BEWARE the measurement trap that produced a WRONG first recommendation here:
#   differencing UNSMOOTHED renders gave z16 -> z17 at 8.5%, which argues loudly
#   for z17 on false grounds. --filter 11 strips most sub-11 m variation BEFORE
#   the hillshade, so any zoom sample must smooth first or it measures detail
#   that never reaches the output. Same trap for any future country.
#
#   MIND THE UNITS. A z17 pixel here is 0.77 GROUND metres, not the 1.194 m that
#   -tr says: Mercator pixels shrink as cos(lat), and at 49.8 degN that is 0.645.
#
# DATUM: NO GRID SHIFT EXISTS, AND NONE IS NEEDED. Checked with projinfo —
#   EPSG:2169 -> EPSG:4326 offers exactly two candidate operations:
#
#     LUREF to WGS 84 (4)   Molodensky-Badekas   1 m
#     LUREF to WGS 84 (3)   7-parameter Helmert  1 m
#
#   Both are stated at 1 m and NEITHER references an NTv2 grid — there is no
#   Luxembourg equivalent of OSTN15 to install, so the England hazard (PROJ
#   silently falling back to a 2 m Helmert, leaving products ~1.9 m off and
#   creating a mixed-datum trap on partial re-render) cannot arise. A z17 pixel
#   is 0.77 ground metres, so the residual 1 m is ~1.3 px and, unlike England's,
#   it is the best available rather than a fixable mistake. No action.
#
# NO gdal_fillnodata, UNLIKE EVERY OTHER COUNTRY. Measured, not assumed: 60
#   random 1200x1200 windows, 34 of them inland, 49 Mpx. 0.75% of inland pixels
#   are nodata, in 4 of 34 windows, and EVERY void blob is 17k-173k px — open
#   water (Sure reservoir, Moselle, lakes). Not one blob was <= 25 px. So
#   `-md 5` has nothing to fill and would leave the raster byte-identical while
#   costing a read+write per window. Those large water voids must stay nodata so
#   they come out transparent, which is what -md 5 does with them anyway.
#   If a future re-survey introduces speckle voids, restore the step by setting
#   fill_md to 5 below.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING, not a style choice.
#   feature-preserving-smoothing does I/O via the `wbgeotiff` crate, which does
#   not parse the TIFF Predictor tag (317) — fed PREDICTOR=2/3 float data it
#   decodes byte-shuffled deltas as garbage and emits +/-Inf rasters that render
#   as static, WITHOUT erroring. (Documented in shading-it.nu, verified 2026-07-17.)
#   NOTE the national raster written by step 0 IS PREDICTOR=3; that is fine
#   because the smoother never sees it, only the PREDICTOR=1 window cuts.
#
# ALWAYS RUN VIA `conda run -n geo`, NEVER by putting the env's bin on PATH.
#   Doing the latter leaves PROJ_DATA unset, and then gdal_fillnodata.py /
#   gdal_edit.py degrade a PROJCRS to ENGCRS["unnamed"] and the warp fails with
#   "Cannot find coordinate operations". That looks exactly like a GDAL bug and
#   is not one. (Diagnosed the hard way, 2026-08-20.)
#
# Smoothing is 11/16/6/6, the Poland/Croatia/Norway/England 1 m settings — the
#   filter is a pixel count, so 11 px = 11 m on 1 m data.
#
# CONTOURS. smooth/ is not kept, so each window also emits a 2 m downsample of its
#   smoothed DEM into smooth2m/, which contours-lu.nu contours directly.
#   `average` resampling is nodata-aware; bilinear/cubic would blend nodata into
#   its neighbours.
#
# THE GRID ORIGIN IS PINNED at (47500, 55000) — the values the old derived grid
#   computed from this raster's own extent (48880.5, 56965.5 floored to the STEP
#   grid), so every existing tile id is unchanged. It is pinned rather than
#   derived because deriving it was the Belgium trap: a source whose extent
#   grows moves every window id and a resumable run treats finished tiles as
#   pending. If the national raster is ever re-fetched with a wider extent, this
#   will now fail loudly instead of silently renumbering.
#
# clamp = true, and it is load-bearing here. The source is ONE raster whose
#   extent is not STEP-aligned, so an edge window's -projwin overhangs it.
#   gdal_translate answers a partial overhang by silently returning a SMALLER
#   raster, which breaks the fixed-collar assumption in the crop step.
#
# Resumable at window granularity: a window whose tiles/<id>.tif exists is
# skipped, and an all-nodata window leaves a tiles/<id>.empty marker so it is not
# re-cut on the next run. ~45% of the 759 windows are outside the border and land
# as .empty on the first pass. A window that throws is recorded in failed/ and
# skipped. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-lu.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC      = "/media/martin/18TB/lu/luxembourg_dem_1m.tif"
const SRC_URL  = "/vsicurl/https://download.data.public.lu/resources/bd-l-lidar2024-releve-3d-du-territoire-luxembourgeois/20241223-093912/MNT_Lidar2024.tif"
const DATA_DIR = "/mnt/osm/lu"                   # smooth2m/, tiles/ on NVMe
const EPSG     = "EPSG:2169"                     # LUREF / Luxembourg TM

gdal assert-mounted "/media/martin/18TB"
gdal require-proj $EPSG "shading-lu.nu"

# ── 0. Fetch the national 1 m DEM (one command, ~11 min, 4.8 GB) ──────────────
# Reads the COG's 1 m overview over HTTP range requests; the 0.5 m data is never
# transferred. Skipped if the file is already there.

if ($SRC | path exists) {
    print $"==> ($SRC) exists — reusing \(delete to re-fetch\)"
} else {
    print "==> Fetching 1 m DEM from data.public.lu — 4.8 GB, ~11 min"
    let tmp = $"($SRC).tmp"
    rm -f $tmp
    (gdal_translate
      --config CPL_VSIL_CURL_ALLOWED_EXTENSIONS .tif
      --config GDAL_DISABLE_READDIR_ON_OPEN EMPTY_DIR
      --config GDAL_HTTP_MAX_RETRY 10 --config GDAL_HTTP_RETRY_DELAY 3
      --config GDAL_HTTP_TIMEOUT 120 --config GDAL_CACHEMAX 2048
      -oo OVERVIEW_LEVEL=0
      -ot Float32
      -co COMPRESS=ZSTD -co ZSTD_LEVEL=1 -co PREDICTOR=3
      -co TILED=YES -co BLOCKXSIZE=512 -co BLOCKYSIZE=512
      -co BIGTIFF=YES -co NUM_THREADS=ALL_CPUS
      $SRC_URL $tmp)
    mv $tmp $SRC
}

shading run {
    code:      "lu"
    src:       $SRC
    data_root: $DATA_DIR
    tiles_dir: $"($DATA_DIR)/tiles"
    out_tif:   "/media/martin/18TB/lu/shading.tif"
    nodata:    "-9999"
    zoom:      17                                # MEASURED, then overruled by eye — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     true                              # single raster, unaligned extent — see header
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-lu.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 47500, y0: 55000, id_width: 2}
}
