#!/usr/bin/env nu

# Generate shaded relief for Belgium. Belgium port of shading-lu.nu.
#
# TWO REGIONS, TWO AUTHORITIES, ONE CRS.
#
#   Wallonia  SPW, LiDAR 2021-2022, MNT 1 m, ~0.12 m vertical, CC BY 4.0.
#             Fetched by download-wa.nu as GeoTIFF tiles in EPSG:3812.
#             This is the half that matters: the Ardennes.
#
#   Flanders  Digitaal Vlaanderen, DHMV II, DTM 1 m, flown 2013-2015 at
#             >=8 points/m2, open data, no restrictions. Served live over
#             WCS 2.0.1 — no download step at all, GDAL reads it as a single
#             247000 x 102000 virtual raster. There is no DHMV III.
#
#   IT IS NOT CLIPPED TO THE FLEMISH REGION. Verified 2026-08-30: the WCS
#   returns real terrain over the whole Brussels-Capital enclave (Grand-Place,
#   Uccle, Foret de Soignes all 100% valid) and several km INTO Wallonia
#   (Waterloo, 6 km south of the border, 118-129 m). Outside its coverage it
#   returns -9999 — the same sentinel Wallonia uses — so the unified vrtnodata
#   and the has-data check both behave. Brussels is therefore NOT a hole in the
#   country, which it would have been had DHMV stopped at the region boundary.
#
#   TRANSIENT 502s. The endpoint is behind an Azure Application Gateway and does
#   return "502 Bad Gateway" under load. Per-window failures land in failed/ and
#   are retried on the next run, which is exactly what that machinery is for —
#   but expect a non-zero failed/ count on the first pass of a large run.
#
# EPSG:3812 (ETRS89 / Belgian Lambert 2008) FOR BOTH, DELIBERATELY.
#   Wallonia's 1 m product ships only in 3812, and Flanders' WCS advertises 3812
#   alongside its native 31370. Taking 3812 for both means:
#     - ONE window grid for the whole country, no seam at the language border;
#     - NO datum hazard. projinfo gives exactly one 3812 -> WGS 84 operation and
#       it is a null transform (ETRS89-based). EPSG:31370 (Lambert 72, BD72)
#       offers THREE candidates at 1-5 m, which is the England/OSTN15 trap:
#       PROJ silently picks one and the product lands metres off OSM with no
#       error raised. Do not "simplify" this to 31370.
#
# THE REAL SEAM IS TEMPORAL, NOT SPATIAL. If Flanders is enabled, the two halves
#   were flown 6-8 years apart with different sensors. The CRS matches, so
#   geometry lines up, but expect a radiometric step along the border where one
#   side has newer quarries, embankments and building pads than the other.
#   Nothing here blends it; if it looks bad, that is the reason.
#
# ZOOM=17, MEASURED 2026-08-28 on smoothed data through this exact pipeline.
#   Three 850 m samples, each coarser render resampled onto the z17 grid and
#   differenced (grey levels, 0-255):
#
#     La Roche-en-Ardenne  z15  mean 2.46  p95 11  10.63% of px off by >5
#                          z16  mean 1.26  p95  5   4.20%
#     Rochers de Freyr     z15  mean 2.16  p95 10   9.65%
#                          z16  mean 1.32  p95  5   4.73%
#     Hesbaye plateau      z16  mean 0.45  p95  1   0.62%
#
#   For comparison: Luxembourg z16 was 2.57% and went to z17; England z16 was
#   3.20% and stayed at z16. Wallonia's Ardennes lose MORE than either, and the
#   cost argument that decided England does not apply — Wallonia is 16900 km2,
#   so z17 is roughly 8 GB against 2 GB, not 137 GB against 44 GB.
#
#   The Hesbaye row matters too: over flat Wallonia the zoom is irrelevant
#   (0.62%), so the decision rests entirely on the Ardennes third of the region.
#
#   MEASURE ON SMOOTHED DATA — this is the trap. Differencing unsmoothed renders
#   overstates the case for a finer zoom by about 3x, because --filter 11 strips
#   most sub-11 m variation BEFORE the hillshade is computed. Luxembourg measured
#   8.5% unsmoothed against 2.6% smoothed for the same comparison, and the
#   unsmoothed figure produced a confidently wrong recommendation.
#
# NO gdal_fillnodata, MEASURED (2026-08-27) — same finding as Luxembourg.
#   Random 1200x1200 windows per province, inland only, nodata blobs labelled:
#
#     Brabant Wallon   68 Mpx   3.01% nodata   1 blob <= 25 px
#     Liege            92 Mpx   4.24% nodata   0 blobs <= 25 px
#     Luxembourg (BE) 114 Mpx   1.73% nodata   0 blobs <= 25 px
#
#   Every void is >1000 px (largest ~692k) — water bodies and coverage edges,
#   which must STAY nodata so they come out transparent, and which -md 5 would
#   not touch anyway. The Ardennes provinces were checked specifically because
#   forest canopy was the plausible source of speckle; there is none. So the
#   step would cost a read+write per window across ~2700 windows to fix one
#   pixel cluster in the whole region.
#
#   FLANDERS RE-MEASURED 2026-08-30 before enabling it, because at a quarter of
#   Wallonia's density and a decade older it was a genuinely separate question.
#   27 windows / 27 Mpx inside coverage: 0.0000% nodata, NO voids of any size.
#   The delivered DHMV II raster is already gap-free within its footprint, so
#   the step stays off for both halves. To restore it, set fill_md to 5 below.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# ALWAYS RUN VIA `conda run -n geo`, never by putting the env's bin on PATH —
#   that leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and
#   the warp fails with "Cannot find coordinate operations" hours in.
#
# Smoothing is 11/16/6/6, the PL/HR/NO/EN/LU 1 m settings.
#
# Resumable at window granularity; empty windows leave a .empty marker; failures
# land in failed/ and are retried on the next run. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-be.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const INCLUDE_FLANDERS = true
const FLANDERS_WCS = "WCS:https://geo.api.vlaanderen.be/DHMV/wcs?version=2.0.1&coverage=DHMVII_DTM_1m"
const DATA_ROOT = "/mnt/osm/be"                  # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:3812"                    # ETRS89 / Belgian Lambert 2008
const NODATA    = "-9999"

let DRIVE   = (gdal find-drive)
let SRC_DIR = $"($DRIVE)/be/MNT1M"               # download-wa.nu's Wallonia tiles
let VRT     = $"($DATA_ROOT)/be.vrt"

print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-be.nu"

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/**/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run download-wa.nu first"}
}

# ── Source VRT: Wallonia on disk, Flanders live over WCS ──────────────────────

mkdir $DATA_ROOT

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing \(delete to force a rebuild\)"
} else {
    print $"==> Building national VRT ($VRT) — unified nodata ($NODATA)"
    let wal = (glob $"($SRC_DIR)/**/*.tif")
    print $"  Wallonia: ($wal | length) tiles"

    # ORDER MATTERS: gdalbuildvrt resolves overlaps last-listed-wins, and the two
    # sources DO overlap — DHMV II is not clipped to the Flemish Region, it covers
    # the Brussels enclave and spills several km into Wallonia (verified at
    # Waterloo, 6 km south of the border). Wallonia must therefore be listed LAST
    # so its 2021-2022 data at higher density wins the overlap strip; otherwise
    # 2013-2015 Flemish data would overwrite good Walloon ground and push the age
    # seam kilometres inside Wallonia instead of onto the region boundary.
    #
    # FLANDERS MUST BE PRE-WARPED TO EPSG:3812 BEFORE IT CAN JOIN THE VRT.
    #   GDAL reports the WCS in its native EPSG:31370, and gdalbuildvrt SILENTLY
    #   SKIPS sources whose CRS disagrees with the first one. Listing the raw WCS
    #   alongside the 3812 province rasters produced a VRT containing ONE source
    #   — Flanders alone, all five Wallonia rasters dropped behind a warning
    #   swallowed by the progress bar. (gdal build-vrt now refuses such a VRT
    #   outright, but the warp is still what makes the sources compatible.)
    #
    #   The warp also fixes a datum problem. 31370 is BD72; going straight to
    #   EPSG:3857 offers PROJ nothing better than a 1 m Helmert. Routing through
    #   3812 (ETRS89) uses the official IGN NTv2 grid at 0.01 m — measured
    #   deviation from the Helmert is 0.14 m mean / 0.33 m max, i.e. under half a
    #   z17 pixel, so this is correctness housekeeping rather than a rescue.
    #   Requires be_ign_bd72lb72_etrs89lb08.tif; install with
    #     projsync --file be_ign_bd72lb72_etrs89lb08.tif
    #   It lands in ~/.local/share/proj and works with PROJ_NETWORK=OFF.
    #
    #   -of VRT keeps it lazy: no pixels are fetched until a window is cut.
    let sources = if $INCLUDE_FLANDERS {
        let fl_vrt = $"($DATA_ROOT)/zone_fl.vrt"
        if not ($fl_vrt | path exists) {
            print "  Flanders: warping WCS 31370 -> 3812 \(lazy VRT\)"
            (gdalwarp -q -of VRT -t_srs $EPSG -tr 1 1 -r bilinear
              -srcnodata $NODATA -dstnodata $NODATA
              $FLANDERS_WCS $"($fl_vrt).tmp")
            # SECOND SILENT-SKIP CAUSE, distinct from the CRS one: gdalbuildvrt
            # also drops sources whose band COLOUR INTERPRETATION disagrees with
            # the first input. A warped VRT comes out Undefined while the
            # province GeoTIFFs are Gray, which silently reduced the mosaic to
            # one source again. Force it to match.
            gdal_edit.py -colorinterp_1 gray $"($fl_vrt).tmp"
            mv $"($fl_vrt).tmp" $fl_vrt
        }
        print "  Flanders: zone_fl.vrt — listed FIRST, Wallonia wins overlaps"
        ([$fl_vrt] | append $wal)
    } else {
        print "  Flanders: SKIPPED \(INCLUDE_FLANDERS = false\)"
        $wal
    }

    gdal build-vrt $sources $VRT --extra [-vrtnodata $NODATA] --index $"($DATA_ROOT)/_idx_be"
}

shading run {
    code:      "be"
    src:       $VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/be/shading.tif"
    nodata:    $NODATA
    zoom:      17                                # MEASURED — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off, both halves — see header
    dem_tr:    2                                 # m; what contours-be.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # PINNED, NOT DERIVED. Deriving it (floor(extent_min / STEP) * STEP) was the
    # original design and it is a trap: the moment the VRT grows — adding
    # Flanders moved the minimum x from 542248 to 516991 — the origin moves with
    # it, every window id changes, and a resumable run silently treats thousands
    # of finished tiles as pending. These values sit below the Flanders+Wallonia
    # union (516991, 521173) on the STEP grid. Do NOT change them: doing so
    # renames every tile.
    grid:      {kind: "pinned", x0: 515000, y0: 520000, id_width: 3}
}
