#!/usr/bin/env nu

# Generate shaded relief for Sachsen (Germany) from the Saxon DGM1.
# Third of the German states; port of shading-de-nw.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Sachsen — 4981 GeoTIFF tiles
#   of 2x2 km (2000x2000 px), 1 m, Float32, nodata -9999, EPSG:25833, with an
#   all.vrt already built over them. From geocloud.landesvermessung.sachsen.de
#   (dl-de/by-2-0). download-de-sn.nu documents the throttle that returns 404
#   rather than 429 — do not raise its PAR above 2.
#
# EPSG:25833 IS ETRS89 / UTM 33N, so there is NO datum hazard: the path to
#   EPSG:3857 is a null transform, as for Bayern/NRW's 25832. Note the zone
#   differs from the other two German states — 33N, not 32N.
#
# ZOOM=17, AND HERE IT IS MEASURED, NOT ASSUMED. NRW took z17 on standing
#   preference because it had no terrain to justify it. Saxony does:
#   sample-zoom-de-sn.nu rendered Sächsische Schweiz (tile dgm1_33414_5650, Elbe
#   sandstone) and found 31.42% of pixels off by more than 5/255 between z16
#   upscaled and native z17 — essentially Bayern's Alpine figure (31.90%).
#
#   The window was chosen as the roughest in the state ON PURPOSE: whatever
#   holds on the most detailed terrain holds everywhere flatter. The Erzgebirge
#   is higher (Fichtelberg 1215 m) but glacially rounded; the sandstone has
#   vertical walls and metre-scale structure, which is what a hillshade shows.
#   The measurement was made on SMOOTHED data — differencing unsmoothed renders
#   overstates the finer zoom by about 3x.
#
# NO gdal_fillnodata, MEASURED 2026-09-16. 300 random 1500x1500 windows; the
#   141 that lie wholly inside the state (<=2% nodata) came back at EXACTLY
#   0.0000% — every one of them completely void-free. The provider has already
#   interpolated buildings and vegetation into the ground surface, so unlike
#   AHN5 there is nothing to repair.
#
#   The nodata that does exist is the state boundary plus large water and the
#   Lusatian open-cast mines: sampled interior-ish windows showed void runs with
#   a MEDIAN of 138 m, two orders off the 1 m slivers that justify a fill. Those
#   must stay void — filling them would invent terrain over lakes and across the
#   border. Re-measure per state; do not inherit this setting blindly.
#
# EDGE ARTEFACTS AT THE STATE BORDER ARE ACCEPTED, as for Bayern and NRW:
#   -compute_edges extrapolates where a window has no neighbour across the
#   boundary. Saxony borders Bayern and (eventually) Thüringen/Sachsen-Anhalt/
#   Brandenburg, so once those are in, the fix is to put the neighbours' tiles
#   in the VRT as CONTEXT while still writing only tiles inside this extent.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned at
#   the Saxon extent (278000, 5560000) floored to the STEP grid. Do NOT change
#   these: doing so renames every tile.
#
# SIZE, as built: 3239 tiles over 6279 windows, 6.04 GB mosaic plus overviews =
#   10.86 GB. That is well under the zoom sample's 2.2 MB/km2 extrapolation,
#   which was measured on the sandstone — the worst-compressing terrain here.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-sn.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Sachsen/all.vrt"
const DATA_ROOT = "/mnt/osm/de_sn"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25833"                   # ETRS89 / UTM zone 33N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-sn.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted, and has download-de-sn.nu built all.vrt?"}
}

shading run {
    code:      "desn"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_sn/shading.tif"
    nodata:    "-9999"
    zoom:      17                                # MEASURED on the sandstone — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-sn.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned at the Saxon extent (278000, 5560000) floored to the STEP grid.
    grid:      {kind: "pinned", x0: 277500, y0: 5560000, id_width: 3}
}
