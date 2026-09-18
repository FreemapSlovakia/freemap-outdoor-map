#!/usr/bin/env nu

# Re-sample Bavaria at zoom, on windows CHOSEN TO MATCH other states' terrain.
#
# WHY THIS EXISTS WHEN BAYERN IS ALREADY RENDERED. Bayern's original test gave
#   31.90% at the Watzmann against 1.26% at the Altmühltal karst, and the obvious
#   reading — Alpine rock is simply rougher — was challenged on the grounds that
#   Bavarian DGM1 might just be better data than the other states'. That is a
#   real possibility: grid spacing is not effective resolution, and point
#   density, gridding method and provider-side smoothing all differ between
#   surveys.
#
#   The original four sites cannot settle it, because they span different
#   terrain AND different data at once. These sites can: each is picked so its
#   MEASURED ROUGHNESS matches a window already sampled in another state, which
#   holds terrain roughly constant and leaves the data as the only variable.
#
#     match-thueringen  roughness 0.105, as the Drachenschlucht — which gave 8.63%
#     match-saxony      roughness 0.263, nearest available to the Bastei's 0.300,
#                       which gave 17.73%
#     roughest          roughness 0.640, the roughest tile in a 591-tile sample,
#                       for the upper bound Bayern can actually reach
#
#   If Bavaria lands near those states' figures at matched roughness, the data is
#   equivalent and terrain explains everything. If it lands materially higher,
#   its DGM1 resolves more and the Alpine number was never purely terrain.
#
# ROUGHNESS MATCHING IS NOT MORPHOLOGY MATCHING, and the numbers should be read
#   with that in mind. The 0.105 tile has a median slope of 3.4° against the
#   Drachenschlucht's 15.4° — a flat valley floor with a mountain edge crossing
#   it, which reaches the same high-frequency energy by a different route than a
#   uniformly steep gorge. Treat these as indicative, not as a controlled trial.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-by.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"

gdal require-proj $EPSG "sample-zoom-de-by.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_by"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Bayern"
    out_dir:   $"($DRIVE)/de_by/zoom-sample-matched"
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
    area_km2:  70550                               # Bayern's land area
    lat_min:   47.27
    lat_max:   50.56

    sites: [
        {name: "match-thueringen", lat: 47.63394, lon: 12.17500,
         note: "roughness 0.105, matched to the Drachenschlucht"}
        {name: "match-saxony",     lat: 47.51653, lon: 10.38798,
         note: "roughness 0.263, nearest available to the Bastei's 0.300"}
        {name: "roughest",         lat: 47.65894, lon: 12.82921,
         note: "roughness 0.640, roughest of 591 sampled tiles"}
    ]
}
