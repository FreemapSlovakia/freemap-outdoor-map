#!/usr/bin/env nu

# Generate shaded relief for Brandenburg, Berlin included, from the LGB DGM1.
# Tenth of the German states; port of shading-de-rp.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Brandenburg — 31291 GeoTIFFs
#   of 1x1 km, 1 m, Float32, LZW, nodata -9999, with an all.vrt already built by
#   download-de-bb.nu. Licence dl-de/by-2-0, credit "© GeoBasis-DE/LGB".
#
# BERLIN IS INSIDE THIS RASTER. The enclave is covered continuously, so the one
#   run shades two federal states.
#
# EPSG:25833, THE ONLY ZONE-33 STATE HERE. Everything else German is 32N.
#
# ZOOM=16, MEASURED ACROSS SEVEN WINDOWS. sample-zoom-de-bb.nu, as a share of
#   pixels more than 5/255 from the native z17 render:
#
#                                   z15      z16
#     Niederfinow canal escarpment  19.45%   11.52%
#     Märkische Schweiz margin      10.16%    5.94%
#     Oderbruch escarpment           8.47%    6.69%
#     Kutschenberg, the high point   6.08%    1.28%
#     Spreewald                      4.71%    3.59%
#     Hoher Fläming                  4.70%    1.88%
#
#   z16's worst case is the best margin of any state so far — Rheinland-Pfalz
#   was 22.22% and Sachsen-Anhalt 31.93%, both accepted.
#
# z15 WAS MEASURED AND REJECTED, and the state-wide numbers are why the question
#   even arose: roughness p50 is 0.019 over 31291 rasters, below the 0.011 of
#   the Rhine plain that was Rheinland-Pfalz's floor. Two things settle it
#   against the median. Berlin is the most-viewed ground in the state and its
#   built-up tiles hold 82 of the top 200 roughness ranks, so the capital would
#   be the one German city shaded a level coarse. And Brandenburg borders
#   Sachsen at z17 and Sachsen-Anhalt at z16, so z15 would put a sharpness step
#   along a straight administrative line, which reads as a fault rather than as
#   terrain.
#
# THE THREE ROUGHEST RASTERS IN THE STATE ARE ALL MAN-MADE — the Rüdersdorf
#   limestone quarry at 0.215, the Welzow-Süd lignite mine at 0.140 and
#   Teufelsberg, a hill of war rubble, at 0.130. The roughest natural ground
#   reaches 0.087. Pick sample sites by rendering them, not by rank.
#
# NO gdal_fillnodata, MEASURED 2026-09-26. 400 random 1500x1500 windows; of the
#   224 holding any data, 209 are completely void-free. Of the 15 that are not,
#   11 exceed 20% nodata and straddle the state edge — chiefly the Oder, where
#   Poland supplies nothing — leaving 4 interior windows between 1% and 15%.
#
# 0.00 IS REAL GROUND, NOT A SENTINEL. The Oderbruch lies at and below sea
#   level, so the clamp stays off and no -srcnodata is set; see
#   download-de-bb.nu for the measurement.
#
# THE CRS WAS MIXED AND IS NORMALISED IN THE SOURCE. 9575 of the 31291 rasters
#   shipped with a plain PROJCRS against the majority COMPOUNDCRS, and
#   gdalbuildvrt would have dropped them. download-de-bb.nu stamps them before
#   all.vrt is built; if that VRT is rebuilt by hand, read that script first.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the Brandenburg extent (250000, 5690000) on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-bb.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Brandenburg/all.vrt"
const DATA_ROOT = "/mnt/osm/de_bb"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25833"                   # ETRS89 / UTM zone 33N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-bb.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-de-bb.nu built all.vrt?"}
}

shading run {
    code:      "debb"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_bb/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-bb.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the Brandenburg extent (250000, 5690000) on the STEP grid.
    grid:      {kind: "pinned", x0: 247500, y0: 5687500, id_width: 3}
}
