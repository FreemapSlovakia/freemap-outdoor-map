#!/usr/bin/env nu

# Sample Niedersachsen at three zooms, across five windows spanning the state's
# whole range, so the ZOOM for shading-de-ni.nu is chosen from evidence.
#
# FIVE SITES, NOT ONE. Every sampler before lib/zoomsample.nu took a single
#   hand-typed tile and verified nothing about it — which is how Saxony's
#   "Bastei" sample turned out to be a Dresden suburb 20 km away, and stood for
#   weeks. Sites here are given as lat/lon and their measured roughness is
#   printed, so a window that is not what it claims is obvious immediately.
#
# THE SITES WERE MEASURED, NOT GUESSED, and the guess was wrong. Eight candidate
#   places picked by reputation (Okertal, Wurmberg, the Ith, the Süntel, the
#   Dörenther Klippen…) topped out at 0.076 roughness. Scanning 1199 downloaded
#   tiles found 0.181 — 2.4x rougher — in unremarkable Harz forest west of
#   Clausthal that no list of landmarks would have suggested.
#
# WHAT THE SPREAD IS FOR. Bayern's original test showed the state's own terrain
#   types diverging by a factor of 25 (Alps 31.90%, karst 1.26%), and deciding
#   from one window hides that. Niedersachsen's distribution is narrow — p50
#   0.036, p90 0.049, p99 0.073 — so the Harz is a genuine outlier and the
#   honest question is whether a zoom should be bought for it alone.
#
#     harz-roughest   0.181  the roughest window found anywhere in the state
#     harz-steep      0.157  275 m of relief, median slope 28.4°
#     weserbergland   0.047  the hill country, and what "not flat" usually means here
#     heath           0.045  Lüneburger Heide, Wilseder Berg
#     marsh           0.035  East Frisia, 4 m of relief — the floor
#
# FOR SCALE, all measured on 2 km windows: Thüringen's Drachenschlucht is 0.105
#   and lost 8.63% at z16; Saxony's Bastei is 0.300 and lost 17.73%; Bayern
#   reaches 0.640. Niedersachsen's best sits between the first two, so expect a
#   figure in the low teens and read it against those.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-ni.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"

gdal require-proj $EPSG "sample-zoom-de-ni.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_ni"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Niedersachsen"
    out_dir:   $"($DRIVE)/de_ni/zoom-sample"
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
    area_km2:  47600                               # Niedersachsen's land area
    lat_min:   51.29
    lat_max:   53.90

    sites: [
        {name: "harz-roughest",  lat: 51.82678, lon: 10.24068,
         note: "roughness 0.181 — roughest window found in the state"}
        {name: "harz-steep",     lat: 51.86904, lon: 10.47424,
         note: "roughness 0.157, 275 m relief, median slope 28.4°"}
        {name: "weserbergland",  lat: 51.97000, lon:  9.52000,
         note: "roughness 0.047 — typical hill country"}
        {name: "heath",          lat: 53.17000, lon:  9.95000,
         note: "roughness 0.045 — Lüneburger Heide, Wilseder Berg"}
        {name: "marsh",          lat: 53.40000, lon:  7.30000,
         note: "roughness 0.035, 4 m relief — the flat floor"}
    ]
}
