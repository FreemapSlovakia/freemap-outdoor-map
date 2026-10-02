#!/usr/bin/env nu

# Sample Flanders at three zooms so the ZOOM for a split Belgium is chosen from
# evidence. See sample-zoom-be-wal.nu for why the question is being re-asked.
#
#     Flanders   roughness p50 0.029   relief p50   9.9 m  51.6% under 10 m
#     Denmark    roughness p50 0.025   relief p50  12.5 m  36.3% under 10 m
#
#   Flanders is flatter than Denmark, which is rendered at z16. Belgium's z17
#   was chosen over the country as a whole, where the Ardennes dominate the
#   measurement; half of Flanders has under 10 m of relief in a square
#   kilometre and cannot use the resolution.
#
# BRUSSELS BELONGS TO THIS HALF. The Flemish source covers the Brussels-Capital
#   Region — 18.28 m at the city centre — so a split on the regional boundary
#   puts Brussels here, with Digitaal Vlaanderen's attribution, which is where
#   its pixels actually come from.
#
# Source: FLANDERS.tif, the consolidated Digitaal Vlaanderen DHMV II fetched
#   over WCS, EPSG:3812, 1 m, nodata -9999. Licence "Modellicentie gratis
#   hergebruik" — NOT the CC BY 4.0 that Wallonia carries, which is the reason
#   for splitting at all.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-be-vlg.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:3812"                           # Belgian Lambert 2008

gdal require-proj $EPSG "sample-zoom-be-vlg.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "be_vlg"
    src_dir:   $"($DRIVE)/be/src_vlg"
    out_dir:   $"($DRIVE)/be/zoom-sample-vlg"
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
    area_km2:  13684                               # Flanders + Brussels
    lat_min:   50.68
    lat_max:   51.51

    sites: [
        {name: "antwerpen", lat: 51.26074, lon: 4.32004,
         note: "Antwerp edge — rank 1 at 0.067, slope p90 19.1° on 12.7 m of relief"}
        {name: "voeren",    lat: 50.82046, lon: 5.65182,
         note: "Voeren / Limburg — 65.8 m of relief, slope p90 28.6°, the steepest sampled"}
        {name: "pajottenl", lat: 50.68004, lon: 4.22623,
         note: "Pajottenland — 35.3 m of relief"}
        {name: "brussel",   lat: 50.89210, lon: 4.31305,
         note: "North of Brussels — 46.5 m; Brussels is covered by this source"}
        {name: "kempen",    lat: 51.07768, lon: 5.24923,
         note: "Kempen — 23.2 m of relief"}
        {name: "polder",    lat: 51.09027, lon: 2.88592,
         note: "Coastal polder near Veurne — 4.4 m of relief, the flat end"}
        {name: "noorderkem",lat: 51.32065, lon: 4.33469,
         note: "Northern Kempen — 16.3 m but slope p50 0.7°, almost featureless"}
    ]
}
