#!/usr/bin/env nu

# Generate shaded relief for Norway from the national 1 m LiDAR DTM (Kartverket DTM1).
# Norway port of shading-hr.nu (Croatia 1 m) / shading.nu (Poland 1 m).
#
# Source: /media/martin/18TB/no/DTM1 — 2033 GeoTIFF tiles from the Geonorge Atom
#   download service (see download-no.nu). Verified uniform across all 2033:
#   15010 x 15010 px, Float32, 1 m, nodata -32767 declared on EVERY tile,
#   EPSG:25833 embedded on EVERY tile, LZW, 512x512 blocks, 6 overviews.
#
#   NONE of the Croatian delivery's quirks apply. Stage 0 is a plain gdalbuildvrt:
#   no -a_srs, no -allow_projection_difference, no SimpleSource->ComplexSource
#   rewriting. -vrtnodata -9999 presents one clean sentinel downstream while each
#   source keeps its real -32767 as a per-source <NODATA> (same mechanism the HR
#   script used to unify four sentinels; here it unifies exactly one).
#
#   The source tiles OVERLAP by 10 px: the grid step is 15000 m but each tile is
#   15010 px, i.e. every tile carries a 5 px collar beyond its 15 km cell. The
#   overlapping pixels are identical, so the VRT needs no special handling — but
#   the window grid below is built on the 15 km CELLS, not the tile extents, or
#   windows would drift by 5 m per tile and double-cover the seams.
#
# ZOOM=16, not 17. Measured on a 1.5 km window of Ostmarka (tile 33-124-114,
#   covered by Oslo 10pkt 2024 — among the densest LiDAR in the country, i.e. the
#   best case for a finer zoom): rendering at z17 and at z16-upscaled-to-z17
#   differ by mean 1.07/255, p95 4.0, with 2.4% of pixels off by more than 5
#   levels. Indistinguishable. z15 differs by mean 3.14 with 16.6% of pixels off
#   by >5 — visible loss. The ceiling is the source: DTM1 is a 1 m grid
#   everywhere (point density 2-10 pkt/m2 changes its QUALITY, not its
#   resolution), and feature-preserving-smoothing --filter 11 strips most
#   sub-11 m variation before the hillshade is computed. z16 over Norway is
#   0.78-1.27 ground m/px (Mercator pixels shrink as cos(lat)) — the same ground
#   detail the z17 countries (SK/AT/CZ/SI, all 46-50 degN) get at 0.8 m/px, and
#   the same choice Sweden made with the same latitudes and the same 1 m LiDAR.
#
# WHY THIS SCRIPT DOES NOT USE gdal_retile (it originated here, before the shared
# window grid in lib/shading.nu made it the only pipeline):
#
#   1. DISK. retiled/ + smooth/ for all of Norway is ~1.8 TB against ~670 GB free
#      on the NVMe. So windows are cut from the national VRT on demand, processed,
#      and their intermediates deleted immediately — only tiles/ accumulates.
#      Peak scratch is PARALLEL x one window, not the whole country.
#
#   2. EMPTINESS. Norway's UTM33 bbox is ~1.8 M km2 but only ~324,000 km2 is land.
#      gdal_retile over the national VRT would write ~450,000 tiles, the large
#      majority all-nodata. Deriving windows from the 2033 real tile footprints
#      (each 15 km cell = 5x5 windows of 3 km) gives 50,825 windows that all
#      contain data by construction.
#
#   Cutting with a collar from the NATIONAL VRT (not per source tile) means each
#   window's smoothing and hillshading see real neighbouring data across source
#   tile seams — the property gdal_retile's -overlap gave the other scripts.
#
# CONTOURS. smooth/ cannot be kept (~900 GB), so each window also emits a 2 m
#   downsample of its smoothed DEM into smooth2m/, which contours-no.nu reads
#   directly. That skips the expensive half of consolidation — reading ~900 GB
#   of 1 m data and resampling it (8.5 h on Croatia's old retile pipeline, ~45 h
#   here). The consolidation pass itself still runs. ~225 GB, fits on the NVMe.
#   `average` resampling is nodata-aware; bilinear/cubic would blend nodata into
#   its neighbours.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING, not a style choice.
#   feature-preserving-smoothing does I/O via the `wbgeotiff` crate, which does
#   not parse the TIFF Predictor tag (317) — fed PREDICTOR=2/3 float data it
#   decodes byte-shuffled deltas as garbage and emits +/-Inf rasters that render
#   as static, WITHOUT erroring. (Documented in shading-it.nu, verified 2026-07-17.)
#
# Smoothing is 11/16/6/6, the Poland/Croatia 1 m settings — the filter is a pixel
#   count, so 11 px = 11 m on 1 m data.
#
# Resumable at window granularity: a window whose tiles/<id>.tif exists is skipped,
# and an all-nodata window leaves a tiles/<id>.empty marker so it is not re-cut on
# the next run. Delete tiles/ (or individual outputs) to force a rebuild. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/shading-no.nu

# ONE UNREADABLE SOURCE TILE MUST NOT KILL A 30-HOUR RUN. A window that throws is
#   recorded in failed/ and skipped; because the pending check only looks for
#   .tif and .empty, a later run retries it automatically once the source is
#   repaired. (Learned the hard way: tile 33-125-145 had silent LZW corruption —
#   correct byte count, bad content — and took the run down at 22751/50825.)
#
# THE 2 m CONTOUR DEM NOW COMES FROM THE FILLED RASTER, not from $smooth before
#   the fill. Contouring the unfilled one breaks a line at every void while the
#   hillshade beside it looks continuous, because the hillshade is filled. Any
#   smooth2m/ tile written before this change is unfilled; delete them (not
#   tiles/) to re-emit. tiles/ is unaffected.
#
use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/no"                   # smooth2m/ on NVMe
const EPSG     = "EPSG:25833"                    # ETRS89 / UTM zone 33N
const NODATA   = "-9999"                         # unified sentinel; sources carry -32767
const PARALLEL = 24

const STEP     = 3000                            # m; 5x5 windows per 15 km cell

# The 15 km CELL grid, derived from the tile naming (verified against 33-107-122,
# 33-124-114 and 33-132-158): a tile named 33-E-N spans px [x0-5, x0+15005] where
# x0 = CELL_X0 + (E - E_REF) * 15000, and likewise in y. Cells are contiguous.
#
# Windows are built on the CELLS, not on the tile extents. The tiles are 15010 px
# — a 5 px collar beyond their cell — so using tile extents would drift the grid
# by 5 m per tile and double-cover every seam.
const CELL_X0   = 5430.0                         # west edge of cell E=107
const CELL_YTOP = 6771000.0                      # north edge of cell N=122
const E_REF     = 107
const N_REF     = 122
const CELL_M    = 15000

let DRIVE   = (gdal find-drive)
let SRC_DIR = $"($DRIVE)/no/DTM1"
let VRT     = $"($DATA_DIR)/no.vrt"

print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-no.nu"

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run download-no.nu first"}
}

mkdir $DATA_DIR

# ── 0. National VRT, one unified nodata ───────────────────────────────────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing \(delete to force a rebuild\)"
} else {
    print $"==> Building national VRT ($VRT) — unified nodata ($NODATA)"
    let tiles = (glob $"($SRC_DIR)/*.tif")
    print $"  ($tiles | length) tiles"
    gdal build-vrt $tiles $VRT --extra [-vrtnodata $NODATA] --index $"($DATA_DIR)/_idx_no"
}

# ── 1. Window origins, one per 15 km delivery cell ────────────────────────────

let origins = (
    glob $"($SRC_DIR)/*.tif"
      | each {|f|
          let parts = ($f | path basename | path parse | get stem | split row "-")
          let e = ($parts.1 | into int)
          let n = ($parts.2 | into int)
          {
            id: $"($e)-($n)"
            x0: ($CELL_X0 + (($e - $E_REF) * $CELL_M | into float))
            y1: ($CELL_YTOP + (($n - $N_REF) * $CELL_M | into float))
          }
        }
)

shading run {
    code:      "no"
    src:       $VRT
    data_root: $DATA_DIR
    tiles_dir: $"($DRIVE)/no/tiles"
    out_tif:   $"($DRIVE)/no/shading.tif"
    nodata:    $NODATA
    zoom:      16                                # MEASURED — see header
    parallel:  $PARALLEL
    tmpdir:    "/dev/shm"
    step:      $STEP
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   5                                 # px; 5 m at 1 m
    dem_tr:    2                                 # m; what contours-no.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "tiles", origins: $origins, nx: ($CELL_M // $STEP), ny: ($CELL_M // $STEP)}
}
