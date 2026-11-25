#!/usr/bin/env nu

# Hillshading for Iceland from ÍslandsDEM v1.0 at 10 m (EPSG:3057).
# Port of shading-it-puglia8.nu, the other coarse single-raster source.
# Fetch the input with download-is.nu first.
#
# ONE NATIONAL RASTER, SO NO VRT. 52002 x 37001 at 10 m, Float32, ZSTD,
# x 239990..760010, y 309990..680000 in ISN93 / Lambert 1993.
#
# NODATA IS -3.4028235e+38, the Float32 lowest — not a friendly sentinel, the
# same shape England's tiles have. GDAL reads it from the header, so nothing
# here restates it; `nodata` below is what gets stamped on the contour DEM.
#
# THE SEA IS THE ONLY HOLE. Measured valid share is 53.74% of the grid, and
# Iceland's 103000 km2 of land is 53.5% of this 520 x 370 km box — so the
# composite is essentially complete over land and fill_md has nothing to do but
# tidy the coastline. A much lower valid share on a re-run means the source
# changed, not that Iceland shrank.
#
# ZOOM 13, AND THE GRID OVERSTATES THE DATA. The source sits on a 10 m grid but
# was built as a per-pixel median of ~127 stereo strips, so it is smoother than
# its grid. Degrading three rugged ice-free windows to 20 m and back costs only
# 7-19% of the mean adjacent-cell slope:
#
#     window            native 10 m   via 20 m   ratio
#     Landmannalaugar         2.116      1.981     1.07
#     Thorsmork               2.816      2.440     1.15
#     Dyrfjoll                0.443      0.373     1.19
#
# On that ratio alone z12 (16.75 m) would be the match. Rendered samples at
# z11-z14 were compared by eye instead, and z13 (8.38 m at 64°N) holds structure
# the ratio does not predict. z14 would be 16x the pixels of z12 to describe
# ground a 10 m source cannot.
#
# IT IS A SURFACE MODEL, AND OVER ICE THAT SHOWS. ÍslandsDEM is ArcticDEM:
# WorldView/GeoEye stereo strips at 2 m, co-registered and reduced to a
# per-pixel median. Iceland is ~2% forested so over land a surface model is
# effectively terrain, but ~11% of the country is under ice that moves metres a
# year and each tile blends a median of 127 strips from different years.
# Hillshading absorbs that; contours do not — see contours-is.nu.
#
# MIN IS -28.9 m, MAX 2107.99 m. Iceland has no land 29 m down, so the low tail
# is stereo noise over water; the high end sits just under Hvannadalshnukur's
# real 2110 m, so the median over the stereo strips left no spikes. Neither tail
# needs clipping: hillshading is indifferent to the low one, and no contour is
# drawn below 0 m anyway.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-is.nu

use lib/gdal.nu
use lib/shading.nu

const EPSG      = "EPSG:3057"
const DATA_ROOT = "/mnt/osm/is"

gdal require-proj $EPSG "shading-is.nu"

let DRIVE = (gdal find-drive)
let SRC   = $"($DRIVE)/is/IslandsDEMv1.0_10x10m_isn93_zmasl.tif"

if not ($SRC | path exists) {
    error make {msg: $"($SRC) is missing — run download-is.nu first"}
}

print $"==> drive: ($DRIVE)"

shading run {
    code:      "is"
    src:       $SRC
    unscale:   false                             # Float32 metres, not scaled ints
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/is/is-shading.tif"
    nodata:    "-9999"
    zoom:      13                                # see header — picked on rendered samples
    parallel:  10
    tmpdir:    "/dev/shm"
    step:      25000                             # m; 2500 px at 10 m
    collar:    100                               # 10 px of context for -compute_edges
    crop:      3
    clamp:     true                              # single raster, extent not step-aligned
    fill_md:   5
    dem_tr:    10                                # m; nothing to gain upsampling a 10 m source
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Pinned, never derived from the extent — deriving it was the Belgium trap.
    # Round origin below the data at x 239990 / y 309990.
    grid:      {kind: "pinned", x0: 200000, y0: 300000, id_width: 3}
}
