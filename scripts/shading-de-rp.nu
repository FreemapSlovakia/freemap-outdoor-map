#!/usr/bin/env nu

# Generate shaded relief for Rheinland-Pfalz from the LVermGeoRP DGM1.
# Ninth of the German states; port of shading-de-he.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Rheinland-Pfalz — 21160
#   GeoTIFFs of 1x1 km, 1 m, Float32, LZW, nodata -9999, vintages 2022-2025,
#   with an all.vrt already built by download-de-rp.nu. Licence dl-de/by-2-0,
#   credit "©GeoBasis-DE / LVermGeoRP <Jahr>, dl-de/by-2-0, www.lvermgeo.rlp.de".
#
# ZOOM=16, MEASURED ACROSS SEVEN WINDOWS. sample-zoom-de-rp.nu:
#
#     Hunsrück above the Nahe    22.22%
#     Sauer valley               18.88%
#     Dahner Felsenland          11.92%
#     Eifel maar plateau          6.77%
#     Calmont, Mosel              2.50%
#     Rhine plain                 0.44%
#
#   Below Baden-Württemberg's 23.5% and Sachsen-Anhalt's 31.93%, both of which
#   were rendered at z17 and judged indistinguishable.
#
# STEEPNESS IS NOT ROUGHNESS, and this state is the clearest demonstration of
#   it in the whole set. The Calmont at Bremm is the steepest vineyard in
#   Europe — 18.8° median slope across the window, twice anything else sampled,
#   and 319 m of relief, the most — and it loses 2.50%, second least of
#   anywhere. Its terraces are broad and smooth between their risers. The
#   window that decides the zoom is ordinary slate upland above the Nahe, which
#   looks like nothing and is densely textured at the metre scale.
#
#   Do not expect the Kaiserstuhl result to generalise: terraced vineyard
#   dominated Baden-Württemberg's table at 23.5% because those terraces are
#   many small steps, not a few large ones.
#
# THE DELIVERY IS THE CLEANEST OF THE GERMAN STATES. Scanned over all 21160
#   rasters: none contains a literal 0.00, so there is no undocumented sentinel
#   as in Baden-Württemberg and no zero edge row as in Hessen. The only sub-40 m
#   values in the state are two quarry floors. 50 rasters are 30%+ planar and
#   every one is water — the Rhine, Mosel and Saar, interpolated flat because
#   lidar does not penetrate; the renderer draws water over the shading.
#
# NO gdal_fillnodata, MEASURED 2026-09-25. 400 random 1500x1500 windows; of the
#   218 lying wholly inside the state, ALL 218 are completely void-free and the
#   mean interior nodata is 0.0000%. No other state has managed that.
#
# THE CRS IS COMPOUND — ETRS89 / UTM 32N + DHHN2016 height. A warp to EPSG:3857
#   was tested and gives the right extent, so nothing special is needed here.
#   The 78 rasters that shipped with a plain PROJCRS, and at a half-pixel
#   offset, were repaired by download-de-rp.nu before all.vrt was built; if that
#   VRT is rebuilt by hand, read that script's header first.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the Rheinland-Pfalz extent (292000, 5422000) on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-rp.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Rheinland-Pfalz/all.vrt"
const DATA_ROOT = "/mnt/osm/de_rp"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-rp.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-de-rp.nu built all.vrt?"}
}

shading run {
    code:      "derp"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_rp/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-rp.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the Rheinland-Pfalz extent (292000, 5422000) on the STEP grid.
    grid:      {kind: "pinned", x0: 290000, y0: 5420000, id_width: 3}
}
