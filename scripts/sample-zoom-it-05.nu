#!/usr/bin/env nu

# Sample Italy's two 0.5 m sources at z17 and z18, so the zoom for their
# hillshading is chosen from evidence rather than from the source resolution.
#
# WHY ONLY THESE TWO. Italy is not one DTM: italy.vrt stacks 18 sources that sit
#   on five different zoom grids, from Puglia's 8 m (z14) to Friuli's and
#   Emilia-Romagna's 0.5 m (z18). Each becomes its own hillshading dataset at
#   its own zoom, so each tier needs its own sample. This is the z18 tier:
#   fvg05 (8117 km² surviving) and emilia05 (12976 km²), 21093 km² together.
#
# WHAT IS ACTUALLY BEING ASKED. 0.5 m data can be drawn at z18, but z18 is four
#   times the pixels of z17 for ground that may hold no more information: a
#   0.597 m Mercator pixel is 0.41-0.43 ground m at these latitudes, finer than
#   the lidar's own point spacing in most of the country. The sites below span
#   steep rock to dead-flat plain so the answer is not read off the Alps alone.
#
# NOT italy.vrt. THAT MOSAIC IS ON THE z17 GRID — 1.194 m pixels — and it
#   bilinear-downsamples fvg05 and emilia05 from their native 0.597 m to get
#   there. Sampling it and calling the finer render "z18" compares z17 data with
#   itself upsampled: the difference is near zero by construction and says
#   nothing about what 0.5 m data holds. This builds its own VRT over the two
#   source rasters, which are already on the z18 grid.
#
# THE SOURCE IS Int32 IN 2 mm STEPS, so the window cut unscales to Float32.
#   gdaldem ignores the band's Scale tag and would render heights 500x too
#   large; the smoother reads raw bytes and would see the same. Nodata is left
#   at -2147483648 by -unscale, which still matches the declared value.
#
# SMOOTHING IS 11/16/6/6, THE 1 m COUNTRIES' SETTING, and that is a choice worth
#   knowing about: a filter is a pixel count, so 11 px is 6.6 m here against
#   11 m on 1 m data. It is deliberately light — this sample is about zoom, and
#   smoothing hard enough to erase 0.5 m detail would answer the question by
#   assumption. Tune it separately once the zoom is settled.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-it-05.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:3857"                   # the mosaic is already on the XYZ grid

gdal require-proj $EPSG "sample-zoom-it-05.nu"

let DRIVE = (gdal find-drive)

const SRC = "/run/media/martin/14TB/DEM/it/out"
let VRT = $"($DRIVE)/it/it05-native.vrt"

mkdir $"($DRIVE)/it"
(gdalbuildvrt -q -overwrite -vrtnodata -2147483648 $VRT
   $"($SRC)/fvg05.tif" $"($SRC)/emilia05.tif")
print $"==> z18 source VRT: ($VRT)"

zoomsample run {
    code:      "it05"
    src_dir:   ""                          # unused: src_vrt is given
    src_vrt:   $VRT
    unscale:   true
    out_dir:   $"($DRIVE)/it/zoom-sample-05"
    epsg:      $EPSG
    nodata:    "-2147483648"
    zooms:     [17 18]
    win_m:     2000
    collar:    12                          # 20 px at 0.597 m, enough for an 11 px filter
    crop:      3
    crop_off:  400
    crop_m:    1200
    png_px:    900
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    fill_md:   5
    area_km2:  21093                       # fvg05 8117 + emilia05 12976, surviving
    lat_min:   43.90
    lat_max:   46.60

    sites: [
        {name: "tarvisio",  lat: 46.50500, lon: 13.58000,
         note: "Julian Alps above Tarvisio — fvg05, 740 m, the steep end"}
        {name: "vajont",    lat: 46.26700, lon: 12.33000,
         note: "Vajont — fvg05, 697 m; the 1963 slide scar is a bare rock face"}
        {name: "carso",     lat: 45.71000, lon: 13.79000,
         note: "Carso above Trieste — fvg05, 286 m; dolines are 10-50 m features, the case for z18"}
        {name: "bologna",   lat: 44.30000, lon: 11.20000,
         note: "Appennino above Bologna — emilia05, 289 m, dissected foothills"}
        {name: "rimini",    lat: 43.95000, lon: 12.30000,
         note: "Hills behind Rimini — emilia05, 422 m"}
        {name: "ravenna",   lat: 44.42000, lon: 12.10000,
         note: "Plain near Ravenna — emilia05, 1.7 m; field drains and levees or nothing at all"}
    ]
}
