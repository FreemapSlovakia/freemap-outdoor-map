#!/usr/bin/env nu

# Generate shaded relief for Saarland from the LVGL DGM1.
# The sixteenth and last German state; port of shading-de-sl.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Saarland — 2775 GeoTIFFs of
#   1x1 km, 1 m, Float32, DEFLATE, surveyed 2025, with an all.vrt already built
#   by download-de-sl.nu. Licence dl-de/by-2-0, credit "© GeoBasis DE/LVGL-SL".
#
# ZOOM=16, forced by the neighbour rather than measured: Saarland's only German
#   border is with Rheinland-Pfalz, which is rendered at z16, and the rest of
#   its boundary is France and Luxembourg. Its roughness p50 of 0.045 is the
#   highest of the six states added in this run — more than double
#   Brandenburg's 0.019 — so z16 is comfortably justified in any case.
#
# THE CLEANEST DELIVERY IN THE GERMAN SET. Scanned over all 2775 rasters: one
#   raster contains 0.00 at all, and only 105 pixels of it; none holds ground
#   below -2 m; two are more than 30% planar. Nodata is honestly declared as
#   -9999, the CRS is a plain PROJCRS EPSG:25832 with no compound variant, and
#   every origin sits on the kilometre grid. Nothing needed repairing, which
#   after Hamburg's two nodata markers and Bremen's zone-prefixed eastings is
#   worth recording.
#
# SURVEYED 2025 THROUGHOUT — the newest data of any German state here, against
#   Rheinland-Pfalz's 2022-2024 and Schleswig-Holstein's 2005-2025 split.
#
# RANK 1 IS ALMOST CERTAINLY A SPOIL HEAP. The roughest raster reaches 0.447
#   with 202.6 m of relief but a median slope of only 1.8°, which is not a
#   hillside; it sits in the Saar coalfield near Saarbrücken, where the
#   Bergehalden are. The steepest natural ground is the Saarschleife at
#   49.505 N, 6.548 E — 221 m of relief at a 31° median slope, rank 3.
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
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-sl.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Saarland/all.vrt"
const DATA_ROOT = "/mnt/osm/de_sl"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-sl.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM drive mounted, and has download-de-sl.nu built all.vrt?"}
}

shading run {
    code:      "desl"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_sl/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # MEASURED across seven windows — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # no interior voids — see header
    dem_tr:    2                                 # m; what contours-de-sl.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the extent (308000, 5441000) on the STEP grid.
    grid:      {kind: "pinned", x0: 307500, y0: 5440000, id_width: 3}
}
