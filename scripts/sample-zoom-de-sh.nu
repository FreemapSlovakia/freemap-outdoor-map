#!/usr/bin/env nu

# Sample Schleswig-Holstein at three zooms, across seven windows, so the ZOOM
# for shading-de-sh.nu is chosen from evidence.
#
# THE SURVEY ERA IS A CONFOUNDER HERE AND NOWHERE ELSE. A quarter of the state
#   is 2005-2007 laser scan and the rest 2020-2025, and the two eras differ by
#   a factor of two on the roughness metric:
#
#     2005-2007   n= 4421   p50 0.0177   p90 0.0241
#     2008-2019   n=   30   p50 0.0214   p90 0.0291
#     2020-2025   n=14106   p50 0.0363   p90 0.0477
#
#   Not one 2005-2007 tile reaches the top twenty. That is survey age, not
#   terrain: the old campaign resolves less, so it cannot be rough. A zoom
#   chosen from the state median would therefore be chosen from a mixture of
#   two different instruments. The windows below are modern except one, which
#   is carried precisely to show what the old data looks like beside it.
#
# Ranked over the 18557 rasters that score (of 18564; the rest are all water):
#
#     Kiel, Schwentine mouth   rank      1   0.230   2023
#     Trave valley             rank      2   0.137   2022   slope p90 45.6°
#     Holstein high ground     rank     32   0.087   2023   Bungsberg, 168 m
#     Sylt dunes               rank    103   0.070   2024
#     Itzehoe Geest edge       rank     27   0.090   2005   THE OLD DATA
#     Sachsenwald              rank    638   0.053   2022
#     Wilstermarsch            rank    614   0.053   2025   Germany's lowest land
#
# 0.00 IS REAL GROUND HERE AND IS NOT MASKED, which is the opposite of the
#   decision taken for Mecklenburg-Vorpommern one state east. 5318 rasters
#   contain zeros over 81 million pixels, but only 37 are more than half zero
#   and those are Baltic; the other 4426 carry under 1%, scattered across the
#   whole tile in ground that runs about -1.6 m to +3 m. That is marsh crossing
#   the zero level. 1150 rasters hold 97 million pixels below -2 m, and
#   Germany's lowest land, Neuendorf-Sachsenbande at about -3.5 m, is in this
#   state. `-srcnodata 0` would punch holes through all of it.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-sh.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"                          # ETRS89 / UTM 32N

gdal require-proj $EPSG "sample-zoom-de-sh.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_sh"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Schleswig-Holstein"
    out_dir:   $"($DRIVE)/de_sh/zoom-sample"
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
    area_km2:  15804                               # Schleswig-Holstein
    lat_min:   53.36
    lat_max:   55.06

    sites: [
        {name: "kiel",      lat: 54.36079, lon: 10.34658,
         note: "Schwentine mouth at Kiel — rank 1 of 18557, surveyed 2023"}
        {name: "trave",     lat: 53.98505, lon: 10.88350,
         note: "Trave valley — rank 2, slope p90 45.6°, the steepest measured"}
        {name: "bungsberg", lat: 54.14177, lon: 10.61503,
         note: "Holstein high ground by the Bungsberg, 168 m — rank 32"}
        {name: "sylt",      lat: 54.94157, lon: 8.32096,
         note: "Sylt dunes — rank 103, 40.4 m of relief in blown sand"}
        {name: "itzehoe05", lat: 53.88161, lon: 9.57047,
         note: "Geest edge near Itzehoe — rank 27 but surveyed 2005: the old data"}
        {name: "sachsenwald", lat: 53.53290, lon: 10.42579,
         note: "Sachsenwald — rank 638, ordinary wooded moraine"}
        {name: "wilstermarsch", lat: 53.95433, lon: 9.35812,
         note: "Wilstermarsch — rank 614, Germany's lowest land at about -3.5 m"}
    ]
}
