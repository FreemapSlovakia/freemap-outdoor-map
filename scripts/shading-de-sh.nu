#!/usr/bin/env nu

# Generate shaded relief for Schleswig-Holstein from the LVermGeo SH DGM1.
# Twelfth of the German states; port of shading-de-mv.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Schleswig-Holstein — 18564
#   GeoTIFFs of 1x1 km, 1 m, Float32, DEFLATE, with an all.vrt already built by
#   download-de-sh.nu. Licence CC BY 4.0, credit "© GeoBasis-DE/LVermGeo SH".
#
# ZOOM=16, MEASURED ACROSS SEVEN WINDOWS. sample-zoom-de-sh.nu, as a share of
#   pixels more than 5/255 from the native z17 render:
#
#                              z15      z16
#     Bungsberg high ground    11.87%    9.50%
#     Wilstermarsch            11.86%    7.72%
#     Sylt dunes               10.90%    7.63%
#     Sachsenwald               8.69%    6.83%
#     Trave valley              5.74%    2.44%
#     Schwentine mouth, Kiel    3.74%    2.54%
#
#   Well inside Brandenburg's 11.52% and Rheinland-Pfalz's 22.22%, both taken at
#   z16. z15's worst natural window is 11.87%, closer than in any other state,
#   and was rejected on neighbours: Schleswig-Holstein borders
#   Mecklenburg-Vorpommern and Niedersachsen, both z16, so a level down would
#   put a sharpness step along two straight administrative lines.
#
# TWO SURVEYS, TWENTY YEARS APART. 4421 rasters are 2005-2007 and 14106 are
#   2020-2025, and the eras differ by a factor of two on the roughness metric —
#   p50 0.0177 against 0.0363, with no old tile in the top twenty. That is the
#   instrument, not the ground. The zoom above was therefore chosen from modern
#   windows; the old quarter gets it anyway and simply has less to show.
#
# RANK DOES NOT PREDICT LOSS, and this state is the clearest case. The roughest
#   raster of all 18557 is the Schwentine mouth at Kiel, built ground whose
#   edges are already sharp at any zoom: it loses 2.54%. The window that decides
#   the zoom is ordinary push moraine by the Bungsberg, rank 32.
#
# 0.00 IS REAL GROUND AND IS NOT MASKED — the opposite of the call taken one
#   state east. 5318 rasters contain zeros over 81 million pixels, but only 37
#   are more than half zero and those are Baltic; the other 4426 carry under 1%,
#   scattered through marsh running about -1.6 m to +3 m. 1150 rasters hold 97
#   million pixels below -2 m, and Germany's lowest land is here. A -srcnodata 0
#   would punch holes through all of it.
#
# THE VRT's -9999 IS THE FILL FOR UNCOVERED GROUND, NOT A SENTINEL IN THE DATA.
#   Without it everything outside the state reads 0.00, which is the same value
#   as the Wilstermarsch, and the shading would draw terrain at sea level across
#   Denmark, Hamburg and the North Sea. See download-de-sh.nu.
#
# NO gdal_fillnodata: the delivery has no interior voids to fill — every tile is
#   a complete grid over its own extent, and the 121 squares the portal never
#   served are Wadden Sea and Helgoland, outside the land the map draws.
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
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-sh.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Schleswig-Holstein/all.vrt"
const DATA_ROOT = "/mnt/osm/de_sh"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-sh.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-de-sh.nu built all.vrt?"}
}

shading run {
    code:      "desh"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_sh/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # no interior voids — see header
    dem_tr:    2                                 # m; what contours-de-sh.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (424000, 5912000) on the STEP grid.
    grid:      {kind: "pinned", x0: 422500, y0: 5910000, id_width: 3}
}
