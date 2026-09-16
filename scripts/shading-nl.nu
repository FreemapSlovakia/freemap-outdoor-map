#!/usr/bin/env nu

# Shaded relief for the Netherlands from AHN5 (0.5 m).
# Source: download-nl.nu's all.vrt — EPSG:28992, nodata already restamped -9999.

# STEP is halved because AHN is 0.5 m where every other country here is 1 m.
# The pipeline is tuned for ~2500 px windows; 2500 m would give 5000 px a side,
# four times the pixels. Costs ~26000 windows against Bayern's 10000.

# The 0.5 m source also means the smoothing filter spans 5.5 m rather than 11 m.
# That is wanted: the Dutch signal is dykes, creek ridges and dune relief, mostly
# 5-20 m wide, which an 11 m filter would partly erase.

# ZOOM=17 MEASURED on the Veluwe (sample-zoom-nl.nu): z16 differs from z17 on
# 25.7% of pixels, the same band as Bavaria's Alps (31.9%) and Saxony (31.4%),
# far above England (3.2%) or Wallonia (4.2%) — in a country with no relief.
# Confirmed on the same data: Limburg has 45x the relief of a Flevoland polder
# and LESS pixel-scale roughness. Relief is the wrong proxy; micro-relief is the
# signal, and a coarse zoom destroys exactly that.

# gdal_fillnodata is ON here. AHN keeps only bare-ground points, so buildings,
# bridges, trees and water are all nodata — 5-14% of pixels. -md 10 (5 m at
# 0.5 px) fills 92.8% of it, measured, which is the man-made part; the remaining
# 7.2% is genuine water and must stay nodata or the hillshade renders texture on
# canals and lakes.

# RD New needs no datum grid here — see download-nl.nu.

# Grid origin is pinned below the AHN extent, not derived from it. Changing
# these renames every tile.

# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-nl.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/AHN/dtm_05m/all.vrt"
const DATA_ROOT = "/mnt/osm/nl"                  # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:28992"                   # Amersfoort / RD New

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-nl.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the source drive mounted, and has download-nl.nu finished and built all.vrt?"}
}

shading run {
    code:      "nl"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/nl/shading.tif"
    nodata:    "-9999"                           # the VRT restamps AHN's FLT_MAX
    zoom:      17                                # MEASURED — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      1250                              # m; 2500 px at 0.5 m — see header
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   10                                # px; 5 m at 0.5 m — see header
    dem_tr:    2                                 # m; what contours-nl.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned below the AHN extent (X 10000.., Y 306250..) with room to spare.
    grid:      {kind: "pinned", x0: 0, y0: 250000, id_width: 3}
}
