#!/usr/bin/env nu

# Generate shaded relief for Flanders, one half of a split Belgium.
# Port of shading-be.nu, which rendered both halves as a single z17 product.
#
# WHY BELGIUM IS SPLIT: two sources under DIFFERENT LICENCES. Wallonia's MNT is
#   CC BY 4.0; the Flemish DHMV II is "Modellicentie gratis hergebruik". As one
#   product every Belgian tile credited both, asserting a CC BY licence over
#   Flemish pixels and vice versa. Splitting makes the per-tile attribution true.
#
#   The old product could not simply be cut: shading.tif is JXL at
#   JXL_DISTANCE=3.0, so slicing it would re-encode lossy data a second time.
#   Both source rasters survive on disk, so this re-renders from them instead.
#
# ZOOM=16, DOWN FROM THE 17 BELGIUM CARRIED. sample-zoom-be-vlg.nu, as a share
#   of pixels more than 5/255 from the native z17 render: median 1.98%, range
#   0.86-10.26%. z17 was chosen over Belgium as a whole, where the Ardennes
#   dominate the measurement.
#
#     Flanders  roughness p50 0.029   relief p50 9.9 m   51.6% under 10 m
#
# Source: $DRIVE/be/src_vlg — EPSG:3812, 1 m, nodata -9999.
#   Licence "Modellicentie gratis hergebruik", credit "Digitaal Vlaanderen".
#
# NO gdal_fillnodata: measured off for both halves when Belgium was one product,
#   and the sources have not changed.
#
# THE GRID ORIGIN IS THE SAME AS THE UNSPLIT PRODUCT, and deliberately so. It is
#   pinned below the Flanders+Wallonia union rather than derived, because
#   deriving it moved every window id when Flanders was first added. Keeping
#   (515000, 520000) for both halves means window ids remain comparable across
#   the two and with the old run.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-be-vlg.nu

use lib/gdal.nu
use lib/shading.nu

const DATA_ROOT = "/mnt/osm/be_vlg"
const EPSG      = "EPSG:3812"                    # Belgian Lambert 2008
const NODATA    = "-9999"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

gdal require-proj $EPSG "shading-be-vlg.nu"

let SRC_DIR = $"($DRIVE)/be/src_vlg"
let VRT     = $"($DATA_ROOT)/be_vlg.vrt"

mkdir $DATA_ROOT

if not ($VRT | path exists) {
    let sources = (ls $"($SRC_DIR)/*.tif" | get name)
    if ($sources | is-empty) {
        error make {msg: $"($SRC_DIR) holds no rasters"}
    }
    print $"==> building ($VRT) from ($sources | length) raster\(s\)"
    gdal build-vrt $sources $VRT --extra [-vrtnodata $NODATA] --index $"($DATA_ROOT)/_idx"
}

shading run {
    code:      "be_vlg"
    src:       $VRT
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/be_vlg/shading.tif"
    nodata:    $NODATA
    zoom:      16                                # MEASURED — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   0                                 # MEASURED off — see header
    dem_tr:    2                                 # m; what contours-be-vlg.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    # Same pinned origin as the unsplit product — see header.
    grid:      {kind: "pinned", x0: 515000, y0: 520000, id_width: 3}
}
