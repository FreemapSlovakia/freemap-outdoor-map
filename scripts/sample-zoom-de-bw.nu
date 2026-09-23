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
# THE SITES WERE MEASURED OVER ALL 36493 SCORABLE RASTERS, and every one kept
#   lands in the top 0.4% of the state:
#
#     fridingen    0.547   rank     1   Danube at Fridingen, crags and terraces
#     donautal     0.313   rank    10   Danube gorge at Beuron
#     feldberg     0.224   rank    31   Feldberg, slope p50 22.1°
#     kaiserstuhl  0.178   rank    57   volcanic hills, vineyard terraces
#     wutach       0.165   rank    72   Wutachschlucht
#     albtrauf     0.132   rank   142   Swabian Alb escarpment
#     rheinebene   0.027   rank 24106   Upper Rhine plain: the floor
#
#   Distribution: p50 0.032, p90 0.056, p99 0.098, max 0.547.
#
#   RANK 2 IS A QUARRY and is carried below only to be excluded — a pit cut into
#   gentle farmland near Crailsheim, 0.459 with 85 m of relief. Hand-picked
#   coordinates went wrong too: the Alb escarpment site had to move 5 km,
#   because the name landed on the plateau above the Trauf, flat farmland with
#   a quarry in the corner. Render every candidate before believing its number.
#
# THE FIRST FULL-STATE SCAN WAS WORTHLESS AND LOOKED FINE. Before nodata was
#   understood, its top thirty were all border tiles scoring up to 12.361
#   against a median of 0.032 — half real terrain, half the 0.00 that this
#   delivery writes for out-of-coverage. A roughness metric cannot tell that
#   from a cliff. See download-de-bw.nu.
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
        {name: "fridingen",   lat: 48.02876, lon: 8.91281,
         note: "Danube at Fridingen — ROUGHEST WINDOW IN THE STATE, rank 1 of 36493"}
        {name: "donautal",    lat: 48.04678, lon: 8.97987,
         note: "Danube gorge at Beuron — limestone crags, rank 10"}
        {name: "feldberg",    lat: 47.87174, lon: 8.03049,
         note: "Feldberg, 1493 m — rank 31, slope p50 22.1°"}
        {name: "kaiserstuhl", lat: 48.07547, lon: 7.70445,
         note: "Kaiserstuhl — volcanic hills under vineyard terracing, rank 57"}
        {name: "wutach",      lat: 47.83809, lon: 8.36521,
         note: "Wutachschlucht — incised gorge, southern Black Forest, rank 72"}
        {name: "albtrauf",    lat: 48.51367, lon: 9.46715,
         note: "Swabian Alb escarpment — cliff rim above the plateau, rank 142"}
        {name: "rheinebene",  lat: 48.60000, lon: 8.00000,
         note: "Upper Rhine plain — the flat floor, rank 24106"}
        {name: "quarry",      lat: 49.15742, lon: 10.06294,
         note: "EXCLUDED — rank 2 is a quarry cut into farmland near Crailsheim"}
    ]
}
