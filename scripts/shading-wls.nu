#!/usr/bin/env nu

# Generate shaded relief for Wales from the Welsh Government LiDAR DTM.
# Port of shading-de-mv.nu; England is the nearer relative by datum and
# instrument, but its grid comes from an OS tile index and this source is one
# COG, so the pinned grid of the German states fits better.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Wales — one national COG,
#   191007x233000 px, 1 m, Float32, DEFLATE, with an all.vrt wrapping it built
#   by download-wls.nu. Licence OGL v3.0, credit "Welsh Government".
#
# EPSG:27700, as England.
#
# ZOOM=16, CHOSEN AGAINST THE MEASUREMENT. sample-zoom-wls.nu, as a share of
#   pixels more than 5/255 from the native z17 render:
#
#     Glyderau                 34.70%   31.57%
#     Snowdon massif           29.19%   23.00%
#     Margam opencast          22.70%   15.97%
#     Rhondda valley sides     18.66%   13.78%
#     Pembrokeshire coast      13.11%    7.90%
#     Anglesey                  8.59%    4.32%
#     Cadair Idris              3.64%    2.03%
#
#   The median 13.78% is the highest in this repository — three times Denmark's
#   4.64%, and Glyderau and Snowdon both exceed Rheinland-Pfalz's worst window
#   of 22.22%. z17 would have cost 22 GB against z16's 8 GB. z16 was chosen
#   anyway; this is a deliberate trade, not an oversight, and the first source
#   here where the finer zoom had a real case.
#
#   CADAIR IDRIS IS WHY THE CASE IS NOT CLEAR-CUT: 422.9 m of relief and only
#   2.03% disagreement. Big relief does not imply fine structure — smooth
#   glacial slopes resolve perfectly well at z16, and much of Wales is that
#   rather than the broken ground of the Glyderau.
#
# 0.00 IS REAL GROUND AND IS NOT MASKED, as in Schleswig-Holstein and unlike
#   Denmark. 28 of 775 sampled land windows carry zeros; the ground genuinely
#   below the datum is written as a negative, with sampled minima to -15.87 m.
#   Nodata is declared honestly as -9999. Do not add a second mask.
#
# NOTHING LIES BELOW -20 m, over 775 random land windows — so unlike Saarland
#   there is no spike tile to repair first. The -15.87 m floor is the Margam
#   opencast, at a median slope of 3.7 degrees: a working pit, not a defect.
#
# THE DATUM IS RIGHT HERE AND WRONG IN ENGLAND. EPSG:27700 needs OSTN15 to
#   reach Web Mercator; PROJ falls back silently to a Helmert that is 0.61-1.97 m
#   out across Wales. OSTN15 has been installed box-wide since 2026-08-17, so
#   this product is correct and England's, built earlier, is about 1.9 m wrong.
#   EXPECT A VISIBLE DISAGREEMENT ALONG THE BORDER — roughly 1.3 px at z16 over
#   250 km — until England is rebuilt. See the datum section of contours-en.nu.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the extent (164993, 164000) on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-wls.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Wales/all.vrt"
const DATA_ROOT = "/mnt/osm/wls"                 # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:27700"                   # OSGB36 / British National Grid

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-wls.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-wls.nu built all.vrt?"}
}

shading run {
    code:      "wls"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/wls/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   5                                 # px; 5 m at 1 m, as England
    dem_tr:    2                                 # m; what contours-wls.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (164993, 164000) on the STEP grid.
    grid:      {kind: "pinned", x0: 162500, y0: 162500, id_width: 3}
}
