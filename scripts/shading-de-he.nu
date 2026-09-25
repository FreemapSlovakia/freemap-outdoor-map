#!/usr/bin/env nu

# Generate shaded relief for Hessen from the HVBG DGM1.
# Eighth of the German states; port of shading-de-bw.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Hessen — 22776 GeoTIFFs of
#   1x1 km, 1 m, Float32, LZW, nodata -9999, EPSG:25832, heights DHHN2016,
#   with an all.vrt already built. Assembled by download-de-he.nu from 27
#   district packages. Licence dl-de/zero-2-0, which asks for no attribution;
#   credited anyway as "Geobasisdaten © Hessische Verwaltung für
#   Bodenmanagement und Geoinformation".
#
# ZOOM=16, MEASURED ACROSS SEVEN WINDOWS. sample-zoom-de-he.nu:
#
#     Bergstraße escarpment      14.55%
#     Rheingau vineyard terraces 10.49%
#     Kellerwald / Edersee        9.15%
#     Großer Feldberg, Taunus     6.46%
#     Wasserkuppe, 950 m          4.52%
#     Hessisches Ried             1.71%
#
#   The widest margin of any state so far. Baden-Württemberg's hardest window
#   lost 23.5% and Sachsen-Anhalt's Brocken 31.93%, and both were rendered at
#   z17 and judged indistinguishable; Hessen's hardest loses 14.55%.
#
# THE STATE'S HIGHEST GROUND IS AMONG ITS SMOOTHEST, which is why the sites
#   were scanned for rather than named. The Wasserkuppe at 950 m ranks 20099 of
#   22776 by roughness — below the median — with 299 m of relief in its window,
#   more than either of the two hardest. So do the Feldberg (6720), the
#   Vogelsberg (8004) and the Odenwald (14798). Hessen's uplands are rounded
#   basalt and quartzite with nothing at the metre scale; all of its fine
#   detail is in the Bergstraße escarpment and the Rheingau terraces. A sample
#   picked by reputation would have measured the smoothest ground in the state
#   and reported it as the hardest case.
#
# 56 RASTERS ARE WATER AND TOP THE ROUGHNESS RANKING. Lidar does not penetrate
#   the Rhine or the Main, so the surface is interpolated flat and a TIN's facet
#   edges read to a high-pass filter exactly like terrain. They are 30-76%
#   planar to within a millimetre against 3-5% on real ground, which is how they
#   were told apart. Nothing is done about them: the renderer draws water over
#   the shading.
#
# NO gdal_fillnodata, MEASURED 2026-09-24. 400 random 1500x1500 windows; of the
#   202 lying wholly inside the state, 201 are COMPLETELY void-free and the mean
#   interior nodata is 0.0001%. The single exception holds 39 runs of 3 px —
#   canopy slivers, immaterial at that quantity.
#
# EPSG:25832 IS ETRS89 / UTM 32N, as for every German state here except Sachsen.
#   No datum hazard: the path to EPSG:3857 is a null transform.
#
# NODATA IS AN HONEST -9999, declared in the files. Unlike Baden-Württemberg,
#   which writes 0.00 for out-of-coverage and flags it nowhere, this delivery
#   needs no -srcnodata on the VRT and no fringe prefilter.
#
# THE CRS TRAP IS download-de-he.nu's, NOT THIS SCRIPT'S. 7389 of the 22776
#   rasters ship without a projection and the rest carry it; mixed like that,
#   gdalbuildvrt keeps the majority and skips the rest behind a warning in its
#   progress bar. They are stamped before all.vrt is built. If that VRT is ever
#   rebuilt by hand, stamp first.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the Hessen extent (412000, 5471000) on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-he.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Hessen/all.vrt"
const DATA_ROOT = "/mnt/osm/de_he"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-he.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted, and has download-de-he.nu built all.vrt?"}
}

shading run {
    code:      "dehe"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_he/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-he.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the Hessen extent (412000, 5471000) on the STEP grid.
    grid:      {kind: "pinned", x0: 410000, y0: 5470000, id_width: 3}
}
