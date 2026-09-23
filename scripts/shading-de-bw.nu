#!/usr/bin/env nu

# Generate shaded relief for Baden-Württemberg from the LGL DGM1.
# Seventh of the German states; port of shading-de-st.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Baden-Wuerttemberg — 1 m
#   GeoTIFFs of 1x1 km, Float32, LZW, nodata 0, EPSG:25832, heights
#   DE_DHHN2016_NH, accuracy 0.15 m, with an all.vrt already built.
#   download-de-bw.nu fetches 2 km zips of XYZ ASCII and converts them.
#   Licence dl-de/by-2-0, attribution "Datenquelle: LGL, www.lgl-bw.de,
#   dl-de/by-2-0".
#
# ZOOM=16, MEASURED ACROSS SIX WINDOWS. sample-zoom-de-bw.nu:
#
#     Kaiserstuhl, terraced vineyard   23.50%
#     Wutachschlucht                   16.79%
#     Feldberg, 413 m relief           14.19%
#     Danube gorge at Beuron           10.82%
#     Swabian Alb escarpment            5.80%
#     Upper Rhine plain                 5.69%
#
#   Every figure sits below Sachsen-Anhalt's Brocken at 31.93%, which was
#   rendered at both zooms and judged indistinguishable. The mountains here are
#   mild: the Feldberg loses 14.19% and the roughest window in the state — the
#   limestone gorge at Beuron — only 10.82%.
#
#   THE HIGHEST LOSS IS NOT TERRAIN, BUT IT IS NOT AN ARTEFACT EITHER. The
#   Kaiserstuhl is terraced vineyard from edge to edge, and an engineered riser
#   is the sharp step that survives feature-preserving smoothing and rewards a
#   finer grid. Unlike the quarries and spoil heaps that topped Niedersachsen's
#   and Sachsen-Anhalt's rankings, this is not one window to exclude: the
#   terracing covers the Kaiserstuhl and long stretches of the Rhine valley
#   slopes, and it is ground people walk. It is the one real argument for z17
#   here and it was weighed and declined.
#
# ROUGHNESS DID NOT MISLEAD HERE, WHICH IS THE EXCEPTION. Nothing man-made
#   reached the top of the scan — the Danube gorge holds the first fourteen
#   places. Every site was still rendered and looked at, which is how the Alb
#   escarpment site moved 5 km: the coordinates picked from reputation sat on
#   the plateau above the Trauf, flat farmland with a quarry in the corner.
#
# EPSG:25832 IS ETRS89 / UTM 32N, as for every German state here except Sachsen.
#   No datum hazard: the path to EPSG:3857 is a null transform.
#
# THE SOURCE IS XYZ ASCII AND download-de-bw.nu OWNS THE CONVERSION. Do not
#   point this script at raw .xyz: it reads the .tif that conversion produced.
#   Three traps live in that script's header — cell-centre coordinates, a tile
#   grid on odd eastings and even northings, and nodata written as 0.00.
#
# THE RASTERS CARRY nodata 0; THE VRT PRESENTS -9999. all.vrt is built with
#   `-srcnodata 0 -vrtnodata -9999`, so out-of-coverage is masked at the source
#   and everything downstream sees the -9999 the rest of the pipeline expects.
#   If all.vrt is ever rebuilt by hand, those two flags are not optional: drop
#   them and the state border becomes a 1 km cliff down to sea level, shaded
#   and contoured as though it were ground.
#
# MASKING THE ZEROS IS NOT ENOUGH, HENCE THE PREFILTER. The cells immediately
#   inside the coverage edge hold a partial value between real ground and that
#   zero, so the rim still falls hundreds of metres in one pixel — 780 m to
#   48 m in the Allgäu. feature-preserving-smoothing treats a step that sharp
#   as a feature and keeps it, exactly as it does Italy's pixel-doubled
#   staircase, so erode_nodata_fringe.py drops one pixel of valid data wherever
#   it touches nodata, BEFORE smoothing. Measured on dgm1_32_581_5276: 742
#   fringe pixels, all adjacent to nodata and none isolated, removed for 0.19%
#   of the window's valid pixels.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the Baden-Württemberg extent on the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-bw.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Baden-Wuerttemberg/all.vrt"
const DATA_ROOT = "/mnt/osm/de_bw"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N
const ERODE     = "/home/martin/fm/freemap-outdoor-map/scripts/erode_nodata_fringe.py"
const SYS_PYTHON = "/usr/bin/python3"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-bw.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted, and has download-de-bw.nu built all.vrt?"}
}

shading run {
    code:      "debw"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_bw/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across six windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-bw.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: {|src, dst| ^$SYS_PYTHON $ERODE $src $dst }   # see header

    # Pinned below the Baden-Württemberg extent (about 388000, 5265000) on the
    # STEP grid, with room to spare so a coverage change cannot move it.
    grid:      {kind: "pinned", x0: 385000, y0: 5260000, id_width: 3}
}
