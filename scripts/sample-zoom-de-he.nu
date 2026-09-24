#!/usr/bin/env nu

# Sample Hessen at three zooms, across seven windows, so the ZOOM for
# shading-de-he.nu is chosen from evidence.
#
# HESSEN'S SUMMITS ARE AMONG ITS SMOOTHEST GROUND, which is the opposite of
#   what a list of landmarks would suggest and the reason the sites below are
#   not the obvious ones. Ranked over all 22776 rasters:
#
#     Bergstraße escarpment   rank      4   0.244   slope p90 45.2°
#     Rheingau vineyards      rank      6   0.193   terraced, 229 m relief
#     Kellerwald / Edersee    rank   2695   0.055
#     Großer Feldberg, Taunus rank   6720   0.047   879 m
#     Taufstein, Vogelsberg   rank   8004   0.045   773 m
#     Odenwald                rank  14798   0.035
#     Wasserkuppe             rank  20099   0.026   950 m, THE HIGHEST POINT
#     Hessisches Ried         rank  22401   0.018   2.6 m relief: the floor
#
#   The Wasserkuppe is the highest ground in the state and scores BELOW the
#   median. Hessen's uplands are rounded basalt and quartzite; all its
#   metre-scale detail is in the Rhine gorge and the Bergstraße, steep valley
#   sides under vineyard terracing. Expect the decision to hinge on those two.
#
# 56 RASTERS OF 22776 ARE WATER, NOT DAMAGE. Lidar does not penetrate it, so
#   the Rhine and Main are interpolated flat — 30 to 76% of their pixels are
#   planar to within a millimetre against 3-5% on real ground. They top a
#   roughness ranking because a TIN's facet edges read as terrain. They are
#   left alone: the renderer draws water over the shading.
#
# THE RANKING'S RUNNER-UP IS A BASALT QUARRY and is carried below only to be
#   excluded, as in every state so far.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-he.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"

gdal require-proj $EPSG "sample-zoom-de-he.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_he"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Hessen"
    out_dir:   $"($DRIVE)/de_he/zoom-sample"
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
    area_km2:  21116                               # Hessen's land area
    lat_min:   49.39
    lat_max:   51.66

    sites: [
        {name: "bergstrasse", lat: 49.62964, lon: 8.68843,
         note: "Bergstraße escarpment — steepest natural ground, slope p90 45.2°"}
        {name: "rheingau",    lat: 49.99000, lon: 7.93000,
         note: "Rheingau vineyard terraces — 229 m relief, engineered risers"}
        {name: "kellerwald",  lat: 51.16793, lon: 8.97855,
         note: "Kellerwald above the Edersee — 221 m relief, slope p90 33°"}
        {name: "taunus",      lat: 50.23148, lon: 8.47422,
         note: "Großer Feldberg, 879 m — 391 m relief in the window"}
        {name: "wasserkuppe", lat: 50.49869, lon: 9.93764,
         note: "Wasserkuppe, 950 m — the state's highest, and below median roughness"}
        {name: "ried",        lat: 49.85362, lon: 8.45051,
         note: "Hessisches Ried — 7.5 m relief, the flat floor"}
        {name: "quarry",      lat: 50.29554, lon: 9.13338,
         note: "EXCLUDED — basalt quarry near Büdingen, rank 2 of 22776"}
    ]
}
