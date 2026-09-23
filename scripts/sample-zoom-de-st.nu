#!/usr/bin/env nu

# Sample Sachsen-Anhalt at three zooms, across six windows, so the ZOOM for
# shading-de-st.nu is chosen from evidence rather than from the state's
# reputation as flat country.
#
# THE OUTCOME WAS z16, AGAINST THIS SCRIPT'S OWN NUMBERS. The pixel percentages
#   below are large enough to buy z17 on the Saxony precedent, but the two
#   renders are not distinguishable side by side — see shading-de-st.nu for why
#   the metric over-reports in the Harz.
#
# THE STATE IS TWO STATES. North of the Harz it is the Altmark and the Börde —
#   20 m of relief across 2 km. The Harz corner holds the Brocken at 1141 m, the
#   highest ground in northern Germany, and the Bode gorge cuts 300 m into it
#   with slopes whose p90 is 52°. A single window cannot speak for both, and the
#   zoom has to be bought for the corner that needs it.
#
# ROUGHNESS WAS MEASURED OVER ALL 5465 TILES, not a sample, and the top of that
#   ranking is mostly not terrain. Verified by rendering each candidate:
#
#     0.324  Südharz, Thüringen border   natural, but 15.6% void — unusable window
#     0.253  Bodetal at Thale            THE gorge: crags, ravines, 294 m relief
#     0.191  Brocken massif              granite blockfield, 586-1049 m
#     0.159  near Coswig                 FLAT FARMLAND — a void artefact, see below
#     0.156  near Haldensleben           spoil heap
#     0.125  near Burg                   QUARRY — terraced benches in flat fields
#     0.103  Profen                      active lignite open-cast mine
#
#   Four of the seven are man-made or spurious. Saxony's sampler reached a
#   Dresden suburb the same way and Niedersachsen's reached an open pit. Look at
#   the render before believing the number.
#
# VOIDS MUST BE EXCLUDED FROM THE ROUGHNESS METRIC, NOT FILLED. The first scan
#   filled nodata with the window mean, which puts a cliff at every void edge;
#   the residual reads that cliff as terrain. Flat farmland beside the
#   Brandenburg border scored 0.159 that way — above most of the Harz. The fix
#   is to score only 3x3 neighbourhoods that are entirely valid. ni_scan.py has
#   the unfixed version, so Niedersachsen's published roughness figures are
#   inflated wherever a window touched a border or a lake.
#
# 467 OF 5465 TILES ARE MORE THAN 2% VOID — 8.5%, well above Niedersachsen. Most
#   are state-border tiles clipped to the Land boundary, plus open water. This is
#   coverage geometry, not missing data, and nothing here fills it.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-st.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"

gdal require-proj $EPSG "sample-zoom-de-st.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_st"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Sachsen-Anhalt"
    out_dir:   $"($DRIVE)/de_st/zoom-sample"
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
    area_km2:  20452                               # Sachsen-Anhalt's land area
    lat_min:   50.89
    lat_max:   53.08

    sites: [
        {name: "bodetal",      lat: 51.73067, lon: 11.01279,
         note: "Bode gorge at Thale — 294 m relief, slope p90 51.8°, the state's best case"}
        {name: "brocken",      lat: 51.80841, lon: 10.63908,
         note: "Brocken massif, 586-1049 m — granite blockfield and tors"}
        {name: "selketal",     lat: 51.72966, lon: 11.07067,
         note: "eastern Harz, 213 m relief, slope p50 19.4° — ordinary Harz forest"}
        {name: "saale",        lat: 51.10529, lon: 11.69980,
         note: "Saale escarpment near Naumburg — vineyard terraces, 140 m relief"}
        {name: "altmark",      lat: 52.61612, lon: 11.70320,
         note: "the flat floor — 21.7 m relief, field boundaries and plough lines"}
        {name: "quarry",       lat: 52.17217, lon: 11.44217,
         note: "EXCLUDED from the decision — terraced quarry in flat farmland"}
    ]
}
