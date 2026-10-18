#!/usr/bin/env nu

# Generate shaded relief for Estonia from the Maa- ja Ruumiamet DTM.
# Port of shading-dk.nu, which is the closest analogue: flat country whose sea
# is masked at the VRT rather than declared as nodata.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Estonia — 3240 tiles of
#   4x4 km, 1 m, Float32, DEFLATE, 82 GB, with an all.vrt already built by
#   download-ee.nu. Licence the Estonian open-data licence, which mirrors
#   CC BY 4.0; credit "Maa- ja Ruumiamet".
#
# EPSG:3301 (L-EST97), the first time this projection appears here.
#
# ZOOM=16, MEASURED ACROSS SEVEN WINDOWS. sample-zoom-ee.nu, as a share of
#   pixels more than 5/255 from the native z17 render:
#
#     Baltic Klint             13.42%
#     Pärnu hinterland          6.81%
#     Ontika escarpment         6.34%
#     Haanja uplands            4.18%
#     Narva                     3.86%
#     Tartu                     1.92%
#     Saaremaa                  1.35%
#
#   The median 4.18% is under Denmark's 4.64%, and Estonia is flatter than both
#   Denmark and Flanders, which are z16. z17 would cost 37 GB against 13 GB
#   over 45339 km² of ground whose median relief is 8.3 m per square kilometre.
#
#   THE KLINT IS THE ONE ARGUMENT AGAINST, and it is a line rather than an
#   area: the limestone escarpment along the north coast is the only abrupt
#   landform in the country. Ontika is the same feature a few kilometres east
#   and scores 6.34%; everything inland sits between 1.35% and 4.18%.
#
# ESTONIA IS THE FLATTEST SOURCE HERE. Roughness p50 0.024 against Denmark's
#   0.025 and Wales's 0.045, and 59.9% of land carries under 10 m of relief in
#   a square kilometre, against Denmark's 36.3%.
#
# 0.00 IS THE SEA AND all.vrt ALREADY MASKS IT with -srcnodata 0. The evidence
#   is the strongest of any country in this repository: over 120 raw tiles only
#   0.066% of zero pixels touch a non-zero neighbour, exact 0.00 outnumbers the
#   0.25 m bands either side by 1502 times, and the split across the datum is
#   100:1 — 153014 pixels just above against 1512 just below. Ground crossing
#   sea level would be symmetric. Treating non-zero as land also reproduces the
#   national land area to 0.2%. Do not add a second mask here.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the extent (365000, 6375000) on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-ee.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Estonia/all.vrt"
const DATA_ROOT = "/mnt/osm/ee"                  # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:3301"                    # L-EST97

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-ee.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-ee.nu built all.vrt?"}
}

shading run {
    code:      "ee"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/ee/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   5                                 # px; 5 m at 1 m
    dem_tr:    2                                 # m; what contours-ee.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (365000, 6375000) on the STEP grid.
    grid:      {kind: "pinned", x0: 362500, y0: 6372500, id_width: 3}
}
