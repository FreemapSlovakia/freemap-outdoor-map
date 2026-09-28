#!/usr/bin/env nu

# Sample Brandenburg at three zooms, across seven windows, so the ZOOM for
# shading-de-bb.nu is chosen from evidence.
#
# THIS IS THE FLATTEST STATE MEASURED. Roughness p50 is 0.019 over 31291
#   rasters, against 0.011 for the Rhine plain window that was Rheinland-Pfalz's
#   floor. The roughest NATURAL ground in the whole state scores 0.087, which is
#   exactly what the Calmont scored in Rheinland-Pfalz — and the Calmont moved
#   2.50% of pixels between z16 and z17, second least of anywhere sampled. So
#   z15 is a live option here in a way it has not been for any earlier state,
#   and this page exists to settle that rather than to choose between z16 and
#   z17.
#
# THE THREE ROUGHEST PLACES IN BRANDENBURG ARE ALL MAN-MADE, which is the
#   starkest version of a pattern now seven states old:
#
#     rank 1  Rüdersdorf limestone quarry   0.215
#     rank 3  Welzow-Süd lignite mine       0.140
#     rank 6  Teufelsberg, Berlin           0.130   a hill of war rubble
#
#   Only Rüdersdorf is carried below, and only to be excluded. Berlin's built-up
#   area owns 82 of the top 200 tiles and the Lausitz mining belt another 49, so
#   an unfiltered "roughest tile" list describes the state's industry, not its
#   terrain.
#
# ELEVATION DOES NOT PREDICT ROUGHNESS HERE EITHER. The Kutschenberg is the
#   highest point in Brandenburg at 201 m and ranks 4257 with 7.4 m of relief
#   across its window; the Schorfheide moraine, 100 m lower, ranks 58. Height
#   above sea level and texture at the metre scale are unrelated.
#
# Ranked over all 31291 rasters (void-aware metric, comparable with
# Sachsen-Anhalt, Baden-Württemberg, Hessen and Rheinland-Pfalz but NOT with
# the older states):
#
#     Schorfheide moraine      rank     58   0.087   roughest natural ground
#     Oderbruch escarpment     rank    160   0.065
#     Hoher Fläming            rank   1311   0.044
#     Märkische Schweiz        rank   2706   0.039
#     Kutschenberg 201 m       rank   4257   0.035   the state's high point
#     Spreewald                rank  17814   0.018   the floor
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-bb.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25833"                          # ETRS89 / UTM 33N, not 32N

gdal require-proj $EPSG "sample-zoom-de-bb.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_bb"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Brandenburg"
    out_dir:   $"($DRIVE)/de_bb/zoom-sample"
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
    area_km2:  30545                               # Brandenburg plus Berlin
    lat_min:   51.36
    lat_max:   53.56

    sites: [
        {name: "schorfheide", lat: 52.85348, lon: 13.93817,
         note: "Schorfheide moraine — rank 58, roughest natural ground in the state"}
        {name: "oderbruch",  lat: 52.40777, lon: 14.52223,
         note: "Oderbruch escarpment above Lebus — rank 160, 26.7 m of relief"}
        {name: "flaeming",   lat: 52.05944, lon: 12.54210,
         note: "Hoher Fläming near the Hagelberg — rank 1311, the western upland"}
        {name: "maerkische", lat: 52.52202, lon: 14.07883,
         note: "Märkische Schweiz — rank 2706, glacial meltwater gorges"}
        {name: "kutschenberg", lat: 51.46862, lon: 13.91304,
         note: "Kutschenberg 201 m, the state high point — rank 4257, 7.4 m relief"}
        {name: "spreewald",  lat: 51.86444, lon: 13.93257,
         note: "Spreewald — rank 17814, braided flatland: the floor"}
        {name: "ruedersdorf", lat: 52.48370, lon: 13.81455,
         note: "EXCLUDED — Rüdersdorf limestone quarry, rank 1, 122 m of cut faces"}
    ]
}
