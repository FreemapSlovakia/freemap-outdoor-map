#!/usr/bin/env nu

# Sample Rheinland-Pfalz at three zooms, across seven windows, so the ZOOM for
# shading-de-rp.nu is chosen from evidence.
#
# THIS STATE HAS NO MOUNTAINS AND A GREAT DEAL OF RELIEF ANYWAY. The Erbeskopf
#   is 816 m, but the Mosel, Sauer, Nahe and Rhine have cut 200-300 m gorges
#   into the slate uplands, and the Pfälzerwald is sandstone scarp country. The
#   detail is in valley sides, not summits — the Eifel's maar plateau sits at
#   rank 10606 of 21160, dead median.
#
# Ranked over all 21160 rasters (void-aware metric, comparable with
# Sachsen-Anhalt, Baden-Württemberg and Hessen but NOT with the older states):
#
#     Hunsrück / Nahe          rank      4   0.301
#     Dahner Felsenland        rank      8   0.234   sandstone scarps
#     Sauer valley             rank    319   0.112
#     Calmont, Mosel           rank    776   0.087   slope p50 24.3°
#     Eifel maar plateau       rank  10606   0.040
#     Rhine plain              rank  21148   0.011   the floor
#
#   ROUGHNESS AND STEEPNESS DISAGREE HERE MORE THAN ANYWHERE. The Calmont is
#   the steepest vineyard in Europe — 24.3° median slope across the window,
#   twice anything else sampled — yet ranks 776 because terraced ground is
#   smooth between its risers. Whether that wins a zoom is the question this
#   page exists to answer, and the Kaiserstuhl said yes in Baden-Württemberg.
#
# RANK 19 IS A LIMESTONE QUARRY near Diez, benched from 229 m down to 12 m, and
#   is carried below only to be excluded. Sixth state running.
#
# THE DELIVERY IS CLEAN, WHICH IS WORTH RECORDING because the last three were
#   not. Scanned over all 21160 rasters: ZERO contain 0.00, so there is no
#   undocumented sentinel as in Baden-Württemberg and no zero edge row as in
#   Hessen. The only sub-40 m values in the state are two quarry floors. 50
#   rasters are 30%+ planar and all are water — the Rhine, Mosel and Saar,
#   interpolated flat because lidar does not penetrate.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-de-rp.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"

gdal require-proj $EPSG "sample-zoom-de-rp.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "de_rp"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Rheinland-Pfalz"
    out_dir:   $"($DRIVE)/de_rp/zoom-sample"
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
    area_km2:  19858                               # Rheinland-Pfalz's land area
    lat_min:   48.97
    lat_max:   50.94

    sites: [
        {name: "hunsrueck", lat: 49.79078, lon: 7.46477,
         note: "Hunsrück above the Nahe — rank 4, roughest natural ground found"}
        {name: "pfalz",     lat: 49.13781, lon: 7.77296,
         note: "Dahner Felsenland — rank 8, sandstone scarps and crag lines"}
        {name: "sauer",     lat: 49.84000, lon: 6.38000,
         note: "Sauer valley on the Luxembourg border — rank 319, 207 m relief"}
        {name: "calmont",   lat: 50.09500, lon: 7.11500,
         note: "Calmont at Bremm — Europe's steepest vineyard, slope p50 24.3°"}
        {name: "maar",      lat: 50.17000, lon: 6.85000,
         note: "Eifel maar plateau — rank 10606, the median of the state"}
        {name: "rheinplain", lat: 49.67169, lon: 8.11993,
         note: "Rhine plain — rank 21148, 3.8 m relief: the floor"}
        {name: "quarry",    lat: 50.31885, lon: 8.06590,
         note: "EXCLUDED — limestone quarry near Diez, rank 19, benched to 12 m"}
    ]
}
