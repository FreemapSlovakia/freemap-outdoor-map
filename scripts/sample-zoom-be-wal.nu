#!/usr/bin/env nu

# Sample Wallonia at three zooms so the ZOOM for a split Belgium is chosen from
# evidence. Belgium was rendered as ONE product at z17 — the only z17 in this
# repository — and splitting it by source is the moment to re-ask that, because
# the two halves are not the same country.
#
#     Wallonia   roughness p50 0.035   relief p50  50.0 m   0.4% under 10 m
#     Flanders   roughness p50 0.029   relief p50   9.9 m  51.6% under 10 m
#
#   Wallonia sits at Schleswig-Holstein's roughness, which is rendered at z16.
#
# THE SITES COME FROM A RANDOM SAMPLE OF LAND, NOT A NATIONAL RANKING. 250
#   one-kilometre windows drawn at random from a land mask built off a coarse
#   overview; a rank quoted below is a rank within that sample.
#
# Source: the five Service public de Wallonie province rasters, EPSG:3812,
#   1 m, nodata -9999. Licence CC BY 4.0, credit "Service public de Wallonie
#   (SPW)". This is the half of Belgium that is NOT Digitaal Vlaanderen's.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-be-wal.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:3812"                           # Belgian Lambert 2008

gdal require-proj $EPSG "sample-zoom-be-wal.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "be_wal"
    src_dir:   $"($DRIVE)/be/src_wal"
    out_dir:   $"($DRIVE)/be/zoom-sample-wal"
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
    area_km2:  16901                               # Wallonia
    lat_min:   49.49
    lat_max:   50.81

    sites: [
        {name: "bastogne",  lat: 49.66292, lon: 5.74673,
         note: "Near Bastogne — rank 1 of the sample at 0.081, but only 29.8 m of relief"}
        {name: "semois",    lat: 49.83116, lon: 4.96713,
         note: "Semois country — 165.2 m of relief, slope p90 25.8°"}
        {name: "hautfagnes",lat: 50.44283, lon: 6.07297,
         note: "Toward the Hautes Fagnes — 177.7 m, the most relief sampled, slope p90 35.9°"}
        {name: "ourthe",    lat: 50.51887, lon: 5.51859,
         note: "Ourthe valley — 135.5 m of relief"}
        {name: "ardenne",   lat: 50.48713, lon: 5.87210,
         note: "Eastern Ardenne — 96.1 m of relief"}
        {name: "hainaut",   lat: 50.59447, lon: 3.39732,
         note: "Hainaut — rough 0.073 on only 36.6 m of relief, the farmland end"}
        {name: "hesbaye",   lat: 50.57489, lon: 4.02063,
         note: "Hesbaye plateau — 28.4 m, slope p50 2.0°, the flat end of Wallonia"}
    ]
}
