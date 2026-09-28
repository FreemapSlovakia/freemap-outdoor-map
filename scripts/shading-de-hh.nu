#!/usr/bin/env nu

# Generate shaded relief for Hamburg from the LGV DGM1.
# Port of shading-de-hh.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Hamburg — 880 GeoTIFFs of
#   1x1 km, 1 m, Float32, LZW, survey 2022-03 throughout, with an all.vrt
#   already built by download-de-hh.nu. Licence dl-de/by-2-0, credit "Freie und
#   Hansestadt Hamburg, Landesbetrieb Geoinformation und Vermessung (LGV)".
#
# ZOOM=16, NOT MEASURED BUT FORCED BY THE NEIGHBOURS. Hamburg is enclosed
#   entirely by Schleswig-Holstein and Niedersachsen, both rendered at z16, so
#   any other choice draws a sharpness step around the whole of the city
#   boundary. Its roughness p50 of 0.0445 is in any case above Brandenburg's
#   0.019 and Mecklenburg-Vorpommern's 0.020, both of which were taken at z16
#   on measurement.
#
# 0.00 IS REAL GROUND. 228 of the 880 rasters contain zeros over 6.5 million
#   pixels and NOT ONE raster is all zero, so this is Elbe marsh crossing the
#   zero level rather than a sea fill. No -srcnodata 0 anywhere.
#
# THE DECLARED NODATA IN THE DELIVERY IS -3.4028235e+38 AND MATCHES NO PIXEL.
#   The real void marker is -9999, a fifth of some tiles. download-de-hh.nu
#   stamps every raster and the VRT carries -srcnodata -9999 as well; read that
#   script before rebuilding all.vrt by hand.
#
# THE EXTENT IS 123 km WIDE FOR A 755 km2 CITY, because Neuwerk and Scharhörn
#   in the Wadden Sea belong to Hamburg and sit 100 km west of everything else.
#   Expect a large majority of empty windows.
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
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-hh.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Hamburg/all.vrt"
const DATA_ROOT = "/mnt/osm/de_hh"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-hh.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-de-hh.nu built all.vrt?"}
}

shading run {
    code:      "dehh"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_hh/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # no interior voids — see header
    dem_tr:    2                                 # m; what contours-de-hh.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (466000, 5916000) on the STEP grid.
    grid:      {kind: "pinned", x0: 465000, y0: 5915000, id_width: 3}
}
