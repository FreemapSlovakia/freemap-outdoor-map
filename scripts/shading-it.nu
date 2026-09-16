#!/usr/bin/env nu

# Generate shaded relief for Italy from the national 5 m HRDTM.
# Italy port of shading.nu (which targets Poland's 1 m GUGiK DTM).
#
# Source: /mnt/osm/it/it.tif — ONE 22 GB file, 195520 x 257218 px, Float32,
# 5 m pixels, EPSG:6875 (RDN2008 / Italy zone N-E), nodata = -9999.
#
# Differences from the Poland script, and why:
#
#  * Source is a single raster, not a tile tree, so there is no source VRT step —
#    the window grid cuts it.tif directly.
#
#  * nodata is a proper -9999, not Poland's overloaded 0. Poland's WCS returned 0
#    both for out-of-coverage AND for genuine 0.00 m coastal terrain, which forced
#    the speck-list / heal machinery. None of that applies here. gdal_fillnodata is
#    still run per tile to close small interior voids (they would otherwise show as
#    transparent specks in the relief); large sea regions stay nodata → transparent.
#
#  * PREDICTOR=1 on the window DEM is LOAD-BEARING, not a style choice. Our
#    feature-preserving-smoothing does I/O via the `wbgeotiff` crate, which does not
#    parse the TIFF Predictor tag (317) at all — it inflates the DEFLATE stream and
#    returns the bytes as-is. Fed PREDICTOR=3 data those bytes are still byte-shuffled
#    deltas, so they decode as float garbage and the tool emits ±Inf / f32::MAX
#    rasters that render as static. It does NOT error: read_band_f32 returns Ok.
#    it.tif itself is PREDICTOR=3, so the window cut is what launders it. Do not
#    "optimise" this to PREDICTOR=3. (Verified 2026-07-17.) The shared library owns
#    this now — CO_DEM in lib/gdal.nu.
#
#  * ZOOM=15, not 16. EPSG:3857 metres inflate by 1/cos(lat); Italy spans 36-47°N,
#    so z15 = 4.78 3857-m/px lands at ~3.3-3.9 ground m/px against a 5 m source
#    (1.3-1.5x oversampled). z14 would be ~6.5-7.7 ground m/px — undersampled by the
#    same factor, throwing away real detail. No zoom lands on 5 m; z15 is the safe
#    side of the miss. z16 would be 4x the pixels for pure interpolation.
#
#  * DE-DOUBLING RUNS BEFORE SMOOTHING, and dropping it is a silent regression.
#    Italy's HRDTM is a mosaic; the Dolomites and much of the high Alps are 10 m
#    data nearest-neighbour-upsampled into the 5 m grid, so every 2x2 block of
#    "5 m" pixels holds one real value. That reads as staircase contours and
#    terraced hillshading. dedouble_dem.py detects those patches by their
#    exact-equality signature and reconstructs the true 10 m surface there,
#    leaving genuine 5 m LiDAR untouched.
#
#    IT MUST RUN BEFORE THE SMOOTHER, not after: feature-preserving smoothing
#    treats a staircase as a FEATURE and will preserve it at any setting. And it
#    must run on the window, not on the final mosaic, so that both the hillshade
#    and the contours (which read smooth2m/) get the repaired surface.
#
#    It uses the SYSTEM python3, NOT the geo env — it needs scipy, which the geo
#    env does not have. It only reads and writes DEFLATE, no JXL, so it needs
#    nothing else from GDAL.
#
#  * Smoothing is --filter 9 --norm_diff 15 --num_iter 5 --max_diff 5, softer than
#    Poland's 11/16/6/6 — a filter is a pixel count, so 11 px is 11 m on 1 m data but
#    55 m on 5 m data. Measured on a Umbria sample (42.56°N 12.66°E): 9/15/5/5 moves
#    the surface 0.38 m mean / 1.7 m p99, Poland's 11/16/6/6 moves 0.43 m / 2.0 m —
#    both negligible vs a 10 m contour interval, so this is an aesthetic call, not a
#    correctness one. Raise the filter if the relief still reads as noisy.
#    NOTE: parts of the source are contour-interpolated and carry corduroy striping
#    and TIN facets. Feature-preserving smoothing treats those as FEATURES and will
#    not remove them at any setting — a low-pass prefilter would be needed instead.
#
# CONVERTED FROM gdal_retile TO THE SHARED WINDOW GRID (scripts/lib/shading.nu).
#   retiled/ and smooth/ are gone: each window is cut from it.tif on demand,
#   smoothed in /dev/shm and deleted on the way out, which removes two full
#   copies of the country from disk. It also gains the .empty markers, the
#   failed/ retry path, the band-count guard and the VRT-drop check that the
#   retile scripts never had.
#
#   STEP IS 12500 m, NOT 2500. The step is in METRES and the pipeline is tuned
#   for ~2500 px windows; on a 5 m source that is 12500 m. Do not copy the 2500
#   from the 1 m countries.
#
#   dem_tr IS 5, NOT 2. The contour DEM is the smoothed window at the source
#   resolution — downsampling a 5 m source to 2 m would invent detail, and
#   contours-it.nu already contoured 5 m data.
#
#   TILE IDS ARE NEW and smooth2m/ did not exist before, so this needs a FULL
#   RE-RENDER. Delete tiles/, retiled/ and smooth/ before the first run.
#
# THE GRID ORIGIN IS PINNED at (6575000, 3920000), below the raster extent
#   (6577750, 3923600). Deriving it from the extent was the Belgium trap: a
#   source whose extent grows moves every window id and a resumable run treats
#   finished tiles as pending. Changing these renames every tile.
#
# Resumable at window granularity; tiles/<id>.tif is skipped if present. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/shading-it.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC      = "/mnt/osm/it/it.tif"            # one 22 GB raster, 5 m, Float32
const DATA_DIR = "/mnt/osm/it"                   # smooth2m/, tiles/ on NVMe
const EPSG     = "EPSG:6875"                     # RDN2008 / Italy zone (N-E)

# The system python3, not the geo env's — dedouble_dem.py needs scipy.
const DEDOUBLE   = "/home/martin/fm/freemap-outdoor-map/scripts/dedouble_dem.py"
const SYS_PYTHON = "/usr/bin/python3"

gdal require-proj $EPSG "shading-it.nu"

if not ($DEDOUBLE | path exists) {
    error make {msg: $"($DEDOUBLE) not found — it repairs the pixel-doubled 10 m patches and must not be skipped silently; see the header"}
}

shading run {
    code:      "it"
    src:       $SRC
    data_root: $DATA_DIR
    tiles_dir: $"($DATA_DIR)/tiles"
    out_tif:   $"($DATA_DIR)/shading.tif"
    nodata:    "-9999"
    zoom:      15                                # 5 m source — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      12500                             # m; 2500 px at 5 m — see header
    collar:    30                                # m; 6 px at 5 m
    crop:      3                                 # px
    clamp:     true                              # single raster, unaligned extent
    fill_md:   5                                 # px; closes small interior voids
    dem_tr:    5                                 # m; source resolution — see header
    smooth:    {filter: 9, norm_diff: 15, num_iter: 5, max_diff: 5}
    prefilter: {|src, dst| ^$SYS_PYTHON $DEDOUBLE $src $dst }   # see header

    grid:      {kind: "pinned", x0: 6575000, y0: 3920000, id_width: 3}
}
