#!/usr/bin/env nu

# Generate shaded relief for Denmark from Danmarks Højdemodel / Terræn. First
# country outside Germany in this series; port of shading-de-mv.nu, which is the
# closest analogue because its sea is masked the same way.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Denmark — 50013 GeoTIFFs of
#   1x1 km, 1 m, Float32, DEFLATE, resampled from 0.4 m, with an all.vrt already
#   built by download-dk.nu. Licence CC BY 4.0, credit "Klimadatastyrelsen".
#
# EPSG:25832, as most of Germany.
#
# ZOOM=16, MEASURED ACROSS SEVEN WINDOWS. sample-zoom-dk.nu, as a share of
#   pixels more than 5/255 from the native z17 render:
#
#     West Jutland dunes       19.22%   14.62%
#     Sjælland moraine         10.25%    4.90%
#     Rold Skov edge            7.78%    5.99%
#     Bornholm                  5.51%    2.47%
#     Stevns Klint              5.50%    4.64%
#     Lammefjord                5.14%    2.90%
#     Himmerland                4.57%    2.27%
#
#   The median 4.64% is below Mecklenburg-Vorpommern's worst window, and this is
#   the flattest ground in the series: national roughness p50 0.025 against
#   Schleswig-Holstein's 0.036.
#
#   THE DUNE COAST IS THE ONE REAL LOSS. 14.62% at a slope p95 of 21.6° is
#   wind-rippled sand, the finest texture in the country and the only place z17
#   would visibly earn its keep. It is a narrow strip against 42933 km², and
#   z17 would cost 45 GB against 17 GB.
#
# 0.00 IS THE SEA AND all.vrt ALREADY MASKS IT with -srcnodata 0. Measured over
#   800 random rasters: of 125 million zero pixels only 0.176% touch a non-zero
#   neighbour, so they are a few enormous blobs and not scattered ground, and
#   exact 0.00 outnumbers the 0.25 m bands either side of it by 23 times. Do not
#   add a second mask here.
#
#   THE POLDERS SURVIVE THAT MASK because they are not written as zero:
#   Lammefjord runs -7.65 m to -1.41 m, and the shading must keep it.
#
# NO gdal_fillnodata, AND NOTHING TO FILL. Voids in this delivery arrive as
#   whole tiles, never as speckle: all 28 all-void rasters were found by file
#   size and rewritten to sea by download-dk.nu, and no partial void appeared in
#   the 800-raster sample. So fill_md is 0 because inland speckle does not
#   exist here, not because it was measured and rejected as in M-V.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the extent (441000, 6049000) on the STEP grid.
#
# THE MOSAIC IS 453000 x 354000 px, the largest in the series — 182 x 142
#   windows at STEP 2500, against Brandenburg's 121 x 108.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-dk.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Denmark/all.vrt"
const DATA_ROOT = "/mnt/osm/dk"                  # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-dk.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-dk.nu built all.vrt?"}
}

shading run {
    code:      "dk"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/dk/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # nothing to fill — see header
    dem_tr:    2                                 # m; what contours-dk.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (441000, 6049000) on the STEP grid.
    grid:      {kind: "pinned", x0: 440000, y0: 6047500, id_width: 3}
}
