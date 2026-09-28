#!/usr/bin/env nu

# Sample Mecklenburg-Vorpommern at three zooms, across seven windows, so the
# ZOOM for shading-de-mv.nu is chosen from evidence.
#
# THE ROUGHEST GROUND HERE IS NATURAL, which breaks a run of seven states in
#   which a quarry, a mine or a spoil heap topped the table. Rank 1 is the
#   Granitz on Rügen — wooded moraine hills above the Baltic, 60.4 m of relief
#   and a median slope of 26.5°, the steepest ground measured in any of the
#   northern states. Ranks 9, 10 and 12 are the Arkona and Jasmund cliff lines.
#
# THE COAST IS WHY THIS STATE IS NOT BRANDENBURG. Inland it is the same glacial
#   lowland: roughness p50 0.020 against Brandenburg's 0.019, and the Helpter
#   Berge, the state's 179 m high point, rank 2886 of 6085. But the cliffed
#   coast reaches 0.141, well above Brandenburg's roughest natural ground at
#   0.087, and a cliff is exactly the feature a coarse zoom destroys.
#
# Ranked over all 6407 rasters, 6085 of which score (the rest are all sea):
#
#     Granitz, Rügen           rank      1   0.141   s50 26.5°
#     Kap Arkona               rank      9   0.087   cliff line
#     Darß dunes               rank     57   0.054
#     Feldberg lake district   rank   1090   0.030
#     Helpter Berge 179 m      rank   2886   0.021   the state high point
#     Müritz shore             rank   3288   0.019   59% planar: open water
#
# THE METRIC SAMPLES THE MIDDLE 512 m OF EACH TILE, and MV's tiles are 2 km
#   where every earlier state's were 1 km, so it sees 6.5% of each tile's area
#   rather than 26%. A narrow feature can fall outside it: the Königsstuhl,
#   Germany's most famous chalk cliff, ranks only 173 because the sampled
#   window sits on the plateau behind it. Rank is a shortlist, not a verdict.
#
# THE KÖNIGSSTUHL SITE IS A MEASURED POSITION, NOT A PUBLISHED ONE. Its map
#   coordinate put the window a kilometre inland, on the plateau again. An
#   east-west profile through the DEM finds the real edge: 110 m of chalk out
#   to E 413400, 63 m at E 413600, void from E 413800. The site below is
#   centred on that, so the window spans plateau, cliff and sea.
#
# 0.00 IS THE BALTIC AND IS ALREADY MASKED by -srcnodata 0 in all.vrt: 941 of
#   6407 rasters contain zeros and several are 100% zero, 1.69 billion pixels
#   in all. The scan treats 0.00 as void for the same reason, so sea does not
#   score as very smooth land and drag the median down.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-mv.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25833"                          # ETRS89 / UTM 33N, as Brandenburg

gdal require-proj $EPSG "sample-zoom-de-mv.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_mv"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Mecklenburg-Vorpommern"
    out_dir:   $"($DRIVE)/de_mv/zoom-sample"
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
    area_km2:  23295                               # Mecklenburg-Vorpommern
    lat_min:   53.11
    lat_max:   54.69

    sites: [
        {name: "koenigsstuhl", lat: 54.57117, lon: 13.65887,
         note: "Königsstuhl chalk cliff, Jasmund — 110 m plateau to sea in under 300 m"}
        {name: "granitz",   lat: 54.40131, lon: 13.65980,
         note: "Granitz, Rügen — rank 1, wooded moraine above the Baltic"}
        {name: "jasmund",   lat: 54.57956, lon: 13.53017,
         note: "Jasmund interior — rank 10, slope p50 24.7°, the steepest median measured"}
        {name: "arkona",    lat: 54.66823, lon: 13.43393,
         note: "Kap Arkona — rank 9, cliff line on the northern tip"}
        {name: "darss",     lat: 54.45448, lon: 12.48561,
         note: "Darß dunes — rank 55, 6.6 m of relief in blown sand"}
        {name: "feldberg",  lat: 53.33773, lon: 13.42302,
         note: "Feldberg lake district — rank 1090, kettle-and-kame with 44 m relief"}
        {name: "mueritz",   lat: 53.43495, lon: 12.75701,
         note: "Müritz shore — rank 3288, 59% planar: the floor"}
    ]
}
