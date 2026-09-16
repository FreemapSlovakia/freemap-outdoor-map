#!/usr/bin/env nu

# Generate shaded relief for Nordrhein-Westfalen (Germany) from the NRW DGM1.
# Second of the German states; port of shading-de-by.nu.
#
# GERMANY IS DONE STATE BY STATE, one shading + contours product each, keyed
#   de-<state> on the ISO 3166-2:DE code. NRW follows Bayern: the data was
#   likewise already on disk, and at 34110 km2 it is under half of Bayern
#   (62 Gpx against 131 Gpx), so it is a cheap second state. The terrain is
#   mostly lowland, with the Eifel, Sauerland and Teutoburger Wald carrying
#   whatever relief there is to show.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/North Rhine-Westphalia —
#   35863 GeoTIFF tiles of 1x1 km, 1 m, Float32, nodata -9999, EPSG:25832, with
#   an all.vrt already built over them. From geobasis.nrw.de (dl-de/zero-2-0,
#   i.e. no attribution legally required — but we credit it anyway).
#
# EPSG:25832 IS ETRS89 / UTM 32N, so there is NO datum hazard: the path to
#   EPSG:3857 is a null transform, as it was for Wallonia's 3812. Contrast
#   Belgium's Flanders half (EPSG:31370 on BD72), which needed the IGN NTv2
#   grid. Nothing to install here.
#
# ZOOM=17 BY STANDING PREFERENCE, NOT MEASURED FOR NRW. Bayern's z17 was earned
#   by the Alps (31.90% of pixels off by >5 at z16); NRW has no comparable
#   terrain, and Bayern's own header warns in as many words not to assume the
#   Alpine figure transfers to the flatter states. The honest expectation here
#   is a z16-vs-z17 difference in the 1-3% band, i.e. the same "subtle" range
#   that England, Luxembourg and Wallonia landed in.
#
#   It is set to 17 anyway because that is the explicit call — no compromise on
#   quality, and consistency across the German states matters more than the
#   disk: NRW is under half of Bayern, so even at z17 this costs roughly 15 GB
#   of final.tif against Bayern's 34 GB.
#
#   If that trade is ever revisited, measure it properly first: render a few
#   representative windows (Eifel, Sauerland, Teutoburger Wald, and a Muensterland
#   or Lower-Rhine flat for contrast) at z16, resample onto the z17 grid, and
#   difference — and do it on SMOOTHED data. Differencing unsmoothed renders
#   overstates the case for the finer zoom by about 3x (Luxembourg: 8.5%
#   unsmoothed against 2.6% smoothed for the same test).
#
# NO gdal_fillnodata, MEASURED 2026-09-08. 60 random 1200x1200 windows, 43 of
#   them inland, 61.9 Mpx. Raw nodata came to 0.8240%, which looked alarming
#   next to Bayern's flat 0.0000% — but all of it sat in exactly two windows,
#   near Eupen (311628, 5585609) and the Hessen border (477426, 5686365). Both
#   are the STATE BOUNDARY, not holes: flood-filling nodata from the window
#   border consumed 510238 of 510238 nodata pixels, leaving 0 px = 0.0000%
#   interior void.
#
#   That distinction is the whole point of the check. Filling boundary nodata
#   would not repair data, it would invent terrain outside NRW and smear it
#   across the seam with the neighbouring state. Only interior voids justify
#   the step, and there are none — so, as for Bayern and Flanders, it is off.
#   Re-measure per state; scripts/nw_voids.py does the border/interior split.
#   To restore it, set fill_md to 5 below.
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
#   a resumable run treated thousands of finished tiles as pending. Pinned at
#   the NRW extent (280000, 5576000) floored to the STEP grid. Do NOT change
#   these: doing so renames every tile.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-nw.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/North Rhine-Westphalia/all.vrt"
const DATA_ROOT = "/mnt/osm/de_nw"               # smooth2m/, tiles/ on NVMe
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-de-nw.nu"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted?"}
}

shading run {
    code:      "denw"
    src:       $SRC_VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/de_nw/shading.tif"
    nodata:    "-9999"
    zoom:      17                                # standing preference — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-de-nw.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned at the NRW extent (280000, 5576000) floored to the STEP grid.
    grid:      {kind: "pinned", x0: 280000, y0: 5575000, id_width: 3}
}
