#!/usr/bin/env nu

# Generate shaded relief for Bayern (Germany) from the Bavarian DGM1.
# First of the German states; port of shading-be.nu.
#
# GERMANY IS DONE STATE BY STATE, one shading + contours product each, keyed
#   de-<state> on the ISO 3166-2:DE code. Bayern first: the data was already on
#   disk, it is the biggest outdoor draw (Alps, Bavarian Forest, Franconian
#   Jura), and at 70550 km2 it is 2.3x Belgium — a real test of the per-state
#   approach before the other fifteen.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Bayern — 71979 GeoTIFF tiles
#   of 1x1 km, 1 m, Float32, LZW, nodata -9999, EPSG:25832, with an all.vrt
#   already built over them. Downloaded via that directory's downloader.nu from
#   geodaten.bayern.de (OpenData, dl-de/by-2-0).
#
# EPSG:25832 IS ETRS89 / UTM 32N, so there is NO datum hazard: the path to
#   EPSG:3857 is a null transform, as it was for Wallonia's 3812. Contrast
#   Belgium's Flanders half (EPSG:31370 on BD72), which needed the IGN NTv2
#   grid. Nothing to install here.
#
# ZOOM=17, MEASURED 2026-09-04 on smoothed data through this exact pipeline.
#   Each coarser render resampled onto the z17 grid and differenced:
#
#     Berchtesgaden / Watzmann   z15 mean 10.81  p95 37  50.86% of px off by >5
#                                z16 mean  5.90  p95 21  31.90%
#     Bavarian Forest, Gr. Arber z16 mean  1.47  p95  5   4.50%
#     Danube plain, Ingolstadt   z16 mean  0.83  p95  3   2.38%
#     Altmuehltal karst          z16 mean  0.60  p95  2   1.26%
#
#   THE ALPS SETTLE IT AND IT IS NOT CLOSE. 31.90% against England's 3.2%,
#   Luxembourg's 2.6% and Wallonia's 4.2% — an order of magnitude more than any
#   terrain handled so far. Steep rock, couloirs and scree hold detail at 1 m
#   that survives --filter 11 and cannot be represented at z16's 1.6 ground
#   metres. A z17 pixel here is 0.81 ground metres against a 1 m source, i.e.
#   mildly oversampling, which is the right side to err on.
#
#   Note how strongly this varies WITHIN one state: the karst at 1.26% would
#   have been perfectly happy at z16. Do not assume the Alpine figure transfers
#   to the flat northern states — measure each, on SMOOTHED data. Differencing
#   unsmoothed renders overstates the case for a finer zoom by about 3x
#   (Luxembourg: 8.5% unsmoothed against 2.6% smoothed for the same test).
#
# NO gdal_fillnodata, MEASURED 2026-09-04. 60 random 1200x1200 windows, 31 of
#   them inland, 45 Mpx: 0.0000% nodata, NO voids of any size. The delivered
#   Bavarian DGM1 is gap-free within its footprint, as Flanders' DHMV II was.
#   (Luxembourg and Wallonia had voids, but every one was open water and none
#   was <= 25 px, so the step was dropped there too.) DGM1 is a different
#   product from a different authority, so this was measured rather than
#   assumed — do the same for each new state. To restore it, set fill_md to 5
#   below.
#
# EDGE ARTEFACTS AT THE STATE BORDER ARE ACCEPTED FOR NOW. Rendering a state in
#   isolation means -compute_edges extrapolates where a window has no neighbour
#   across the border, leaving a seam along every internal German boundary. The
#   fix is to include neighbouring states' tiles in the VRT as CONTEXT while
#   still only writing tiles inside this state's extent — cheap, but it needs
#   the neighbours downloaded first. Revisit once more states are in.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the Bayern extent (498000, 5235717) on the STEP grid. Do NOT change these:
#   doing so renames every tile.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-by.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Bayern/all.vrt"
const DATA_ROOT = "/mnt/osm/de_by"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-by.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted?"}
}

shading run {
    code:      "deby"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_by/shading.tif"
    nodata:    "-9999"
    zoom:      17                                # MEASURED — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-by.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned at the Bayern extent floored to the STEP grid.
    grid:      {kind: "pinned", x0: 495000, y0: 5235000, id_width: 3}
}
