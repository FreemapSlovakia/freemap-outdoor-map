#!/usr/bin/env nu

# Generate shaded relief for Niedersachsen from the Lower Saxon DGM1.
# Fifth of the German states; port of shading-de-th.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Niedersachsen — 49708 Cloud-
#   Optimized GeoTIFFs of 1x1 km, 1 m, Float32, LZW, nodata -9999, EPSG:25832,
#   with an all.vrt already built. Assembled by download-de-ni.nu from the LGLN
#   STAC API, newest vintage per tile. Licence CC-BY-4.0, attribution LGLN.
#
# ZOOM=16, MEASURED ACROSS FIVE WINDOWS. sample-zoom-de-ni.nu rendered the Harz,
#   the Weserbergland, the Lüneburger Heide and an East Frisian marsh, and every
#   natural window agrees within about a point:
#
#     Harz, 354 m relief, slope p50 23.7°    3.07%
#     East Frisian marsh, 4.6 m relief       3.39%
#     Lüneburger Heide                       2.13%
#     Weserbergland, 294 m relief            2.12%
#
#   The most mountainous ground in the state loses 3.07% at z16, so z17 is not
#   defensible here: at 47600 km2 it would cost about 46 GB against z16's 14 GB —
#   more than Bayern, Sachsen and Thüringen put together. z15 was the real
#   question rather than z17, and it loses 18-20% on both hill sites, so z16 is
#   the floor rather than a compromise.
#
#   DO NOT COMPARE THOSE FIGURES WITH THE ONES IN THE OTHER STATES' HEADERS.
#   They were produced by a metric that differenced the colour bands while
#   ignoring alpha, and alpha carries the shading strength — flat ground renders
#   nearly transparent, so the old metric compared pixels nobody sees. The marsh
#   above scored 44.65% that way against 3.39% composited. Every figure quoted
#   for Thüringen, Sachsen, Bayern, England, Luxembourg and Wallonia predates the
#   fix and is inflated by an unknown factor.
#
# THE ROUGHEST WINDOW IN THE STATE IS A QUARRY, and it was excluded. Scanning
#   1199 tiles by high-frequency residual returned an open pit — terraced
#   benches, haul roads, sub-metre steps — scoring 21.61% at z16, seven times any
#   natural site, with z15 barely worse at 26.48% because a finer grid does not
#   resolve a bench edge either. Roughness finds man-made steps before it finds
#   mountains; Saxony's sampler reached a Dresden suburb by the same route. When
#   picking sample windows for a hiking map, look at the render before trusting
#   the number.
#
# NO gdal_fillnodata, MEASURED 2026-09-21. 400 random 1500x1500 windows; of the
#   207 lying wholly inside the state, 206 are COMPLETELY void-free and the mean
#   interior nodata is 0.0081%. The single exception has void runs of the full
#   window width — open water crossing it, which must stay void.
#
# EPSG:25832 IS ETRS89 / UTM 32N, as for Bayern, NRW and Thüringen. No datum
#   hazard: the path to EPSG:3857 is a null transform.
#
# MIXED VINTAGES ARE PRESENT AND ACCEPTED. 41% of tile keys had more than one
#   vintage in the catalogue and download-de-ni.nu takes the newest, so adjacent
#   tiles can come from surveys years apart. Measured: 2.1% of tile edges are
#   cross-vintage and they step by a median of 6 cm (p99 0.66 m), which is below
#   anything a hillshade at 1.5 m/px shows. Nothing here compensates for it.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned at
#   the Lower Saxon extent (342000, 5682000) floored to the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-ni.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Niedersachsen/all.vrt"
const DATA_ROOT = "/mnt/osm/de_ni"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-ni.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted, and has download-de-ni.nu built all.vrt?"}
}

shading run {
    code:      "deni"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_ni/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across five windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-ni.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned at the Lower Saxon extent (342000, 5682000) floored to the STEP grid.
    grid:      {kind: "pinned", x0: 340000, y0: 5680000, id_width: 3}
}
