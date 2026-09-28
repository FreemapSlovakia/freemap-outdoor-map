#!/usr/bin/env nu

# Generate shaded relief for Bremen from the Landesamt GeoInformation DGM1.
# Port of shading-de-hb.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Bremen — 583 GeoTIFFs of
#   1x1 km, 1 m, Float32, DEFLATE, with an all.vrt already built by
#   download-de-hb.nu. Licence CC BY 4.0, credit "Landesamt GeoInformation
#   Bremen".
#
# ZOOM=16, NOT MEASURED BUT FORCED BY THE NEIGHBOUR. Bremen sits wholly inside
#   Niedersachsen, which is rendered at z16, so its entire boundary is a seam
#   with one state. Its roughness p50 of 0.0294 also sits above Brandenburg's
#   0.019 and Mecklenburg-Vorpommern's 0.020, both taken at z16 on evidence.
#
# TWO CITIES 60 km APART, SURVEYED TWO YEARS APART — Bremerhaven 2015 and
#   Bremen 2017 — and their source archives disagreed about how to write a
#   coordinate: Bremerhaven prefixes the UTM zone onto the easting and Bremen
#   writes cell corners where Bremerhaven writes centres. download-de-hb.nu
#   repairs each separately; read it before rebuilding all.vrt by hand, because
#   an unrepaired Bremerhaven tile lands 2900 km east.
#
# 0.00 IS REAL GROUND. 128 of the 583 rasters contain zeros and NOT ONE raster
#   is all zero, so this is Weser marsh crossing the zero level. No
#   -srcnodata 0.
#
# THE FLOOR IS ABOUT -12 m, which is dock basin and dredged fairway rather than
#   natural ground; expect contours below zero.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the extent (424000, 5912000) on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-hb.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Bremen/all.vrt"
const DATA_ROOT = "/mnt/osm/de_hb"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-hb.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-de-hb.nu built all.vrt?"}
}

shading run {
    code:      "dehb"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_hb/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # no interior voids — see header
    dem_tr:    2                                 # m; what contours-de-hb.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (465000, 5873000) on the STEP grid.
    grid:      {kind: "pinned", x0: 465000, y0: 5872500, id_width: 3}
}
