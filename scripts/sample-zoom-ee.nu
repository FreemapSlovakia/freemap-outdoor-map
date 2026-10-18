#!/usr/bin/env nu

# Sample Estonia at three zooms, across seven windows, so the ZOOM for
# shading-ee.nu is chosen from evidence.
#
# ESTONIA IS THE FLATTEST SOURCE IN THIS REPOSITORY. Measured over 571 random
#   land windows: roughness p50 0.024 and relief p50 8.3 m per square
#   kilometre, with 59.9% of land under 10 m of relief.
#
#     Estonia   0.024   8.3 m   59.9% under 10 m
#     Flanders  0.029   9.9 m   51.6%
#     Denmark   0.025  12.5 m   36.3%
#     Wales     0.045 107.6 m    2.2%
#
#   Denmark and Flanders are both rendered at z16, so the expectation here is
#   z16 or lower, and the measurement is what settles it.
#
# THE SITES COME FROM A RANDOM SAMPLE OF LAND, NOT A NATIONAL RANKING, with two
#   carried deliberately: the Baltic Klint, which is the one piece of abrupt
#   ground in the country, and the Haanja uplands in the south, which hold the
#   high point. A rank quoted below is a rank within the 571-window sample.
#
# 0.00 IS THE SEA AND all.vrt ALREADY MASKS IT with -srcnodata 0, which is the
#   Denmark decision. The evidence here is the strongest of any country: across
#   120 raw tiles only 0.066% of zero pixels touch a non-zero neighbour, exact
#   0.00 outnumbers the two 0.25 m bands either side by 1502 times, and the
#   split across the datum is 100:1 — 153014 pixels in (0, +0.25] against 1512
#   in [-0.25, 0). Real ground crossing sea level would be symmetric; this is a
#   shoreline clamp. Separately, treating non-zero as land reproduces the
#   national land area to 0.2%, which it could not do if zero were terrain.
#
# THE MINES ARE NOT IN THE SAMPLE AND THAT IS EXPECTED. The 25 m national model
#   holds 5099 cells below zero, down to -13.58 m — the oil-shale open-casts in
#   the north-east, about 3.2 km². A 571-window sample covers roughly 1% of the
#   country and found nothing under -20 m, which bounds the typical and says
#   nothing about the extreme. Read the contour table for the floor.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-ee.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:3301"                           # L-EST97

gdal require-proj $EPSG "sample-zoom-ee.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "ee"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Estonia"
    out_dir:   $"($DRIVE)/ee/zoom-sample"
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
    area_km2:  45339                               # Estonia
    lat_min:   57.52
    lat_max:   59.68

    sites: [
        {name: "narva",     lat: 59.35713, lon: 28.19495,
         note: "Near Narva — rank 1 of the sample at 0.053, 22.7 m of relief"}
        {name: "klint",     lat: 59.33552, lon: 27.68724,
         note: "Baltic Klint — slope p90 20.9°, the steepest ground sampled"}
        {name: "ontika",    lat: 59.37550, lon: 28.19720,
         note: "Coastal escarpment — 36.6 m of relief, the most in the sample"}
        {name: "haanja",    lat: 57.89649, lon: 27.05407,
         note: "Haanja uplands — floor already at 72 m, the high ground of Estonia"}
        {name: "tartu",     lat: 58.41773, lon: 26.81846,
         note: "Near Tartu — rough 0.053 on only 8.4 m of relief"}
        {name: "parnu",     lat: 58.44878, lon: 23.94146,
         note: "Pärnu hinterland — 11.0 m of relief"}
        {name: "saaremaa",  lat: 58.95480, lon: 22.44450,
         note: "Saaremaa — 5.4 m of relief, the flat end"}
    ]
}
