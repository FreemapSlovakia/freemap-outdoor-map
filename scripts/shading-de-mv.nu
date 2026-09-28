#!/usr/bin/env nu

# Generate shaded relief for Mecklenburg-Vorpommern from the GeoBasis-DE/M-V
# DGM1. Eleventh of the German states; port of shading-de-bb.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Mecklenburg-Vorpommern —
#   6407 GeoTIFFs of 2x2 km, 1 m, Float32, DEFLATE, with an all.vrt already
#   built by download-de-mv.nu. Licence CC BY 4.0, credit "© GeoBasis-DE/M-V".
#
# EPSG:25833, as Brandenburg. Every other German state here is 32N.
#
# ZOOM=16, MEASURED ACROSS SEVEN WINDOWS. sample-zoom-de-mv.nu, as a share of
#   pixels more than 5/255 from the native z17 render:
#
#                              z15      z16
#     Feldberg lake district   14.57%    5.23%
#     Darß dunes               10.00%    0.62%
#     Granitz, Rügen            9.12%    0.62%
#     Königsstuhl cliff         7.29%    5.33%
#     Kap Arkona                5.05%    2.95%
#     Jasmund interior          2.34%    1.82%
#     Müritz shore              1.32%    0.38%
#
#   5.33% is the widest margin of any state so far — Brandenburg's worst was
#   11.52% and Rheinland-Pfalz's 22.22%, both accepted at z16.
#
# COASTAL WINDOWS UNDERSTATE THEIR LOSS. Where the Baltic is masked both renders
#   are white, so the comparison scores no difference across the sea. The
#   Königsstuhl frame is about a quarter sea, which puts its true land-only
#   figure nearer 7% than 5.33%.
#
# THE ROUGHEST GROUND IS NATURAL, the first time in eight states that no quarry,
#   mine or spoil heap holds rank 1. The Granitz on Rügen reaches 0.150 and the
#   Jasmund and Arkona cliff lines follow. Inland is Brandenburg again:
#   roughness p50 0.020 against 0.019, and the 179 m Helpter Berge ranks 2098
#   of 6085.
#
# 0.00 IS THE BALTIC AND all.vrt ALREADY MASKS IT with -srcnodata 0: 941 of the
#   6407 rasters contain zeros, several are entirely zero, 1.69 billion pixels
#   in all. Land borders are not padded, so nothing inland is lost. Do not add
#   a second mask here.
#
# NO gdal_fillnodata, MEASURED 2026-09-27. 400 random 1500x1500 windows; of the
#   225 holding data, 184 are completely void-free. Of the 41 that are not, 25
#   exceed 20% and are coastal sea; the inland remainder is speckle at or below
#   0.014%.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the extent (206000, 5890000) on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-mv.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Mecklenburg-Vorpommern/all.vrt"
const DATA_ROOT = "/mnt/osm/de_mv"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25833"                   # ETRS89 / UTM zone 33N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-mv.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-de-mv.nu built all.vrt?"}
}

shading run {
    code:      "demv"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_mv/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-mv.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (206000, 5890000) on the STEP grid.
    grid:      {kind: "pinned", x0: 205000, y0: 5887500, id_width: 3}
}
