#!/usr/bin/env nu

# Sample Baden-Württemberg at three zooms, across six windows, so the ZOOM for
# shading-de-bw.nu is chosen from evidence.
#
# THE SOURCE IS XYZ ASCII, NOT GeoTIFF. Each 2 km download holds four 1 km
#   sub-tiles of a million "x y z" lines, 29 MB apiece, which
#   download-de-bw.nu converts to LZW GeoTIFF and deletes. Coordinates are CELL
#   CENTRES (397000.50, not 397000.00) and GDAL's XYZ driver reads that
#   convention correctly on its own — do NOT add the +0.5 shift Thüringen
#   needed, which would move every tile half a metre.
#
# THE TILE GRID SITS ON ODD EASTINGS AND EVEN NORTHINGS. dgm1_32_525_5388
#   exists; 524_5388 and 525_5387 do not. Sachsen-Anhalt is even/even, so a grid
#   generator ported from it returns 404 for every tile — which is exactly what
#   happened on the first attempt here.
#
# THE SITES WERE MEASURED, AND TWO OF SIX HAND-PICKED ONES WERE WRONG. Scanning
#   the downloaded tiles by high-frequency residual moved the Alb escarpment
#   site 5 km — the original coordinates sat on the plateau above it, flat
#   farmland with a quarry — and found better ground in the Black Forest and the
#   Kaiserstuhl than reputation suggested. Roughness measured here (void-aware
#   metric, so comparable with Sachsen-Anhalt's but NOT with the older states'):
#
#     donautal     0.273   Danube gorge at Beuron, limestone crags
#     feldberg     0.155   Feldberg, 413 m relief in the window
#     albtrauf     0.146   Swabian Alb escarpment, cliff rim
#     kaiserstuhl  0.145   volcanic hills, vineyard terraces
#     wutach       0.141   Wutachschlucht
#     rheinebene   —       Upper Rhine plain, 17 m relief: the floor
#
#   Unusually for this pipeline, nothing man-made reached the top of that
#   ranking. Sachsen-Anhalt's top seven held a quarry, a spoil heap and a
#   lignite pit; Baden-Württemberg's is the Danube gorge fourteen tiles deep.
#   Every site below was still rendered and looked at before it was kept.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-bw.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"

gdal require-proj $EPSG "sample-zoom-de-bw.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_bw"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Baden-Wuerttemberg"
    out_dir:   $"($DRIVE)/de_bw/zoom-sample"
    epsg:      $EPSG
    nodata:    "-9999"
    zooms:     [15 16 17]
    win_m:     2000
    collar:    6
    crop:      3
    crop_off:  400
    crop_m:    1200
    png_px:    900
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    fill_md:   5
    area_km2:  35748                               # Baden-Württemberg's land area
    lat_min:   47.53
    lat_max:   49.79

    sites: [
        {name: "donautal",    lat: 48.04678, lon: 8.97987,
         note: "Danube gorge at Beuron — limestone crags, roughest window found"}
        {name: "feldberg",    lat: 47.87174, lon: 8.03049,
         note: "Feldberg, 1493 m — 413 m relief, slope p90 41.7°"}
        {name: "albtrauf",    lat: 48.51367, lon: 9.46715,
         note: "Swabian Alb escarpment — cliff rim above the plateau"}
        {name: "kaiserstuhl", lat: 48.07547, lon: 7.70445,
         note: "Kaiserstuhl — volcanic hills under vineyard terracing"}
        {name: "wutach",      lat: 47.83809, lon: 8.36521,
         note: "Wutachschlucht — incised gorge, southern Black Forest"}
        {name: "rheinebene",  lat: 48.60000, lon: 8.00000,
         note: "Upper Rhine plain — 17 m relief, the flat floor"}
    ]
}
