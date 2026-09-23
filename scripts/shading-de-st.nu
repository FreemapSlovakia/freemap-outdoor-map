#!/usr/bin/env nu

# Generate shaded relief for Sachsen-Anhalt from the Saxony-Anhalt DGM1.
# Sixth of the German states; port of shading-de-ni.nu.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Sachsen-Anhalt — 5465 GeoTIFFs
#   of 2x2 km, 1 m, Float32, LZW, nodata -9999, EPSG:25832, with an all.vrt
#   already built. Assembled by download-de-st.nu from four statewide zips.
#   Licence dl-de/by-2-0, attribution "©LVermGeo Sachsen-Anhalt".
#
# ZOOM=16, DECIDED ON THE RENDERS AND AGAINST THE METRIC. sample-zoom-de-st.nu
#   reports a large share of pixels changing between z16 and z17 — 31.93% on the
#   Brocken, 14.79% in the Bode gorge, and even 3.10% on flat Altmark farmland —
#   which on the Saxony precedent would buy z17. Side by side at the same screen
#   size the two renders are not distinguishable, so z16 stands.
#
#   THE PERCENTAGE IS NOT WRONG, IT IS ANSWERING A DIFFERENT QUESTION. It counts
#   pixels that differ by more than 5/255, and in the Harz most of that
#   difference is metre-scale speckle — granite blockfield and the floor of
#   beetle-killed spruce stands — not landform. Speckle has edges, so
#   feature-preserving-smoothing keeps it, and a finer grid resolves more of it
#   without showing the viewer a hill they could not already see. Do not reopen
#   this on the strength of the number alone; look at the crops first.
#
#   For the same reason the ranking inside that table is not a ranking of
#   terrain. The Bode gorge scores 0.230 roughness against the Brocken's 0.155
#   yet loses half as many pixels: roughness is the energy of fine detail, the
#   loss figure is the share of pixels carrying it, and the gorge is extreme but
#   local in a window that is half smooth plateau.
#
# THE ROUGHNESS SCAN'S TOP SEVEN HELD FOUR MAN-MADE SITES — a quarry near Burg,
#   a spoil heap near Haldensleben, the active lignite pit at Profen, and flat
#   farmland whose score was an artefact of the state border. Niedersachsen's
#   roughest window in the whole state was an open pit and Saxony's "Bastei"
#   sample was a Dresden suburb. Render candidates before trusting them.
#
# NO gdal_fillnodata, MEASURED 2026-09-23. 400 random 1500x1500 windows; of the
#   188 lying wholly inside the state, 187 are COMPLETELY void-free and the mean
#   interior nodata is 0.0064%. The single exception has void runs with a median
#   of 266 px — open water crossing the window, which must stay void, and which
#   fill_md 10 would close 0% of anyway.
#
# EPSG:25832 IS ETRS89 / UTM 32N, as for Bayern, NRW, Thüringen and
#   Niedersachsen. No datum hazard: the path to EPSG:3857 is a null transform.
#
# 40 TILES SHIP WITHOUT A CRS TAG AND ARE ALL IN THE HARZ. gdalbuildvrt drops
#   them with a warning it hides in its progress bar, which would have left the
#   state's only mountains out of the mosaic. download-de-st.nu stamps
#   EPSG:25832 on them and builds through `gdal build-vrt`, which verifies every
#   input made it in. If all.vrt is ever rebuilt by hand, do the same.
#
# MIXED VINTAGES ARE PRESENT AND ACCEPTED, 2009 to 2025, most of it 2019 on.
#   Unlike Niedersachsen there is nothing to choose: the four source zips are a
#   spatial partition with no tile name appearing twice, so each tile has exactly
#   one vintage and neighbours can still differ by years.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned at
#   the Saxony-Anhalt extent (606000, 5646000) floored to the STEP grid.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-st.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Sachsen-Anhalt/all.vrt"
const DATA_ROOT = "/mnt/osm/de_st"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-st.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted, and has download-de-st.nu built all.vrt?"}
}

shading run {
    code:      "dest"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_st/shading.tif"
    nodata:    "-9999"
    zoom:      16                                # DECIDED ON THE RENDERS — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-st.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned at the Saxony-Anhalt extent (606000, 5646000) floored to the STEP grid.
    grid:      {kind: "pinned", x0: 605000, y0: 5645000, id_width: 3}
}
