#!/usr/bin/env nu

# Generate shaded relief for Thüringen (Germany) from the Thuringian DGM1.
# Fourth of the German states; port of shading-de-sn.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Thueringen — 17126 GeoTIFF
#   tiles of 1x1 km, 1 m, Float32, nodata -9999, EPSG:25832, with an all.vrt
#   already built. Assembled by download-de-th.nu from the GDI-Th INSPIRE Atom
#   feed: the 2020-2025 survey wherever it exists, 2014-2019 for the 182 tiles
#   it still lacks. Licence dl-de/by-2-0, attribution "GDI-Th, Freistaat
#   Thüringen" — REQUIRED, as for Saxony and unlike NRW's zero-2-0.
#
# THE SOURCE MIXES TWO VINTAGES, AND download-de-th.nu RECONCILES THEM. The two
#   deliveries use different coordinate conventions half a pixel apart, and the
#   older one arrives as xyz whose unlisted cells read 0 rather than nodata.
#   Both are fixed at download time and every tile was verified to sit on the
#   integer-metre grid; nothing here has to compensate. See that script's header
#   before touching the source directory.
#
# EPSG:25832 IS ETRS89 / UTM 32N — same zone as Bayern and NRW, unlike Saxony's
#   33N. No datum hazard: the path to EPSG:3857 is a null transform.
#
# ZOOM=16, MEASURED, AND THE FIRST GERMAN STATE NOT AT 17.
#   sample-zoom-de-th.nu rendered the Drachenschlucht at Eisenach — chosen by
#   scoring eight candidate sites on high-frequency residual, not by reputation,
#   since relief is the wrong proxy: Kyffhäuser and the Schwarzatal have more
#   relief and are smoother per pixel. Result:
#
#     z15 upscaled to z17:  31.32% of pixels off by more than 5/255
#     z16 upscaled to z17:   8.63%
#
#   8.63% sits well below the ~31% that earned z17 in Bayern and Saxony, and
#   well above the 1-3% band where a finer zoom is plainly wasted. It is a
#   judgement call and the call was z16, for about a third of z17's disk.
#
#   DO NOT READ SAXONY'S 31.42% AS A LIKE-FOR-LIKE COMPARISON. Its sample window
#   is a Dresden suburb, not the Sächsische Schweiz its own header claims — the
#   tile sits 20 km west of the Bastei. Re-running the sampler on the real
#   sandstone gives 17.73%, so the ranking by terrain is genuine and Saxony is
#   about twice as pixel-rough as Thüringen either way:
#
#     Thüringen, forest gorge   8.63%   roughness 0.105
#     Saxony, real sandstone   17.73%   roughness 0.300
#     Saxony, the suburb used  31.42%   roughness 0.045
#     Bavaria, Watzmann        31.90%
#
#   Buildings inflate the figure — the suburb beats the sandstone despite being
#   the smoothest ground of the three — because the DGM1 interpolates them into
#   the surface as flat tops with near-vertical sides, and step edges resolve
#   differently at 0.75 m than at 1.5 m. But terrain drives it too, so this is
#   not merely an artefact. For an outdoor map the terrain number is the one to
#   compare against.
#
# NO gdal_fillnodata, MEASURED 2026-09-18. 400 random 1500x1500 windows; of the
#   204 lying wholly inside the state, 202 are COMPLETELY void-free and the mean
#   interior nodata is 0.0107%. What voids exist are large — median run 30 m,
#   p90 229 m, max 522 m — i.e. water bodies, not canopy slivers. fill_md 20
#   would close 1.6% of them and would bridge lakes with invented terrain.
#
#   This agrees with Bayern, NRW and Saxony, but was established independently:
#   the 2014-2019 half of this source genuinely does carry nodata, so the
#   setting could not be inherited. Re-measure per state.
#
# EDGE ARTEFACTS AT THE STATE BORDER ARE ACCEPTED, as elsewhere. Thüringen is
#   the first state to border TWO that are already rendered — Sachsen to the
#   east and Bayern to the south — so it is the best candidate for the context
#   fix: put the neighbours' tiles in the VRT as context while still writing
#   only windows inside this extent. Not done here; it needs the window grid to
#   read from a multi-state VRT.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned at
#   the Thuringian extent (561000, 5562000) floored to the STEP grid. Do NOT
#   change these: doing so renames every tile.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-th.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Thueringen/all.vrt"
const DATA_ROOT = "/mnt/osm/de_th"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-th.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted, and has download-de-th.nu built all.vrt?"}
}

shading run {
    code:      "deth"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_th/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-th.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned at the Thuringian extent (561000, 5562000) floored to the STEP grid.
    grid:      {kind: "pinned", x0: 560000, y0: 5560000, id_width: 3}
}
