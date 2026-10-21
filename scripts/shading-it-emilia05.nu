#!/usr/bin/env nu

# Shaded relief for Emilia-Romagna at z18, from the region's 0.5 m lidar.
#
# Source: /run/media/martin/14TB/DEM/it/out/emilia05.tif — Emilia-Romagna lidar
#   2023-24 and 2025-26,
#   already warped to EPSG:3857 on the XYZ z18 grid (0.597164 m), Int32 in 2 mm
#   steps, nodata -2147483648. Read-only; it belongs to the Italy source build.
#
# LICENCE not yet recorded. Nothing credits this dataset until a row exists in
#   write-dtm-attribution.nu and in the elevation API's registry.
#
# ITS OWN DATASET, NOT PART OF A NATIONAL `it`. Italy is 18 sources from 18
#   authorities across five zoom grids; one national raster would have to credit
#   all of them on every tile. Each source ships separately, masked by the finer
#   ones above it in the stack — the Wallonia/Flanders split, at scale.
#
# NOT italy.vrt. THAT MOSAIC IS ON THE z17 GRID and bilinear-downsamples this
#   raster from 0.597 m to 1.194 m on the way in. Building z18 from it would
#   throw away exactly the detail z18 exists to carry, silently: the output is
#   the right size and looks right. Cut from the source raster, always.
#
# z18 IS MEASURED, NOT ASSUMED. sample-zoom-it-05.nu compared z17 against z18
#   over six windows. The largest gain of all six was in this region: 29.0% of
#   pixels in the dissected Appennino above Bologna move by more than 5 grey
#   levels of 255. The Ravenna plain gains 4.0%. Median slope predicts the
#   gain; roughness does not.
#
# Int32 IN 2 mm STEPS, so `unscale` is set. gdaldem ignores the band's Scale tag
#   and would hillshade heights 500x too large; the smoother reads raw bytes and
#   would see the same. Nodata pixels survive -unscale unchanged.
#
# STEP IS 1500 m, which is 2512 px on a 0.597 m source — the ~2500 px window the
#   pipeline is tuned for. The step is in metres and does not transfer between
#   sources of different resolution.
#
# THE GRID ORIGIN IS PINNED below the extent (1252344, 5635549) on the step grid.
#   Deriving it from the extent was the Belgium trap: a source whose extent grows
#   moves every window id, and a resumable run then treats finished tiles as
#   pending. Changing these renames every tile.
#
# Resumable at window granularity. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-it-fvg05.nu

use lib/gdal.nu
use lib/shading.nu

const SRC       = "/run/media/martin/14TB/DEM/it/out/emilia05.tif"
const DATA_ROOT = "/mnt/osm/it_emilia05"

let DRIVE = (gdal find-drive)

shading run {
    code:      "it_emilia05"
    src:       $SRC
    unscale:   true                              # Int32, 2 mm steps
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/it/emilia05-shading.tif"
    nodata:    "-9999"
    zoom:      18                                # MEASURED — see header
    parallel:  12
    tmpdir:    "/dev/shm"
    step:      1500                              # m; 2512 px at 0.597164 m
    collar:    12                                # 20 px, clears the 11 px filter
    crop:      3
    clamp:     true                              # single raster, extent not step-aligned
    fill_md:   8                                 # px; ~5 m at 0.597 m
    dem_tr:    2                                 # m; what the contour run reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 939000, y0: 5322000, id_width: 3}
}
