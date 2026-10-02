#!/usr/bin/env nu

# Sample Wales at three zooms, across seven windows, so the ZOOM for
# shading-wls.nu is chosen from evidence.
#
# THE SITES COME FROM A RANDOM SAMPLE OF LAND, NOT A NATIONAL RANKING. 775
#   one-kilometre windows were drawn at random from a land mask built off the
#   COG's coarsest overview — half the rectangle is sea, so sampling the
#   bounding box blind would have spent most of its reads on nodata and put the
#   windows in the water, which is what happened on the first pass at Denmark.
#   A rank quoted below is a rank within that sample.
#
#     Snowdon massif            rank  1   0.433   590.3 m relief, slope p90 48.3°
#     Glyderau                  rank  6   0.160   437.6 m, floor already at 460 m
#     Cadair Idris              rank  8   0.140   391.5 m
#     Rhondda valley sides      rank  4   0.182   212.8 m
#     Margam opencast           rank  2   0.221   the -15.87 m floor, slope p50 3.7°
#     Pembrokeshire coast       rank 12   0.121   cliffs, 90.8% land in frame
#     Anglesey                  rank 15   0.109   the flat end, slope p50 2.1°
#
# WALES IS THE MOST DEMANDING SOURCE IN THIS REPOSITORY SO FAR, which is why
#   the zoom question is worth more here than it was in Denmark. Roughness p50
#   is 0.045 against Denmark's 0.025 and Schleswig-Holstein's 0.036, and relief
#   p50 is 107.6 m per square kilometre against Denmark's 12.5 m. Only 2.2% of
#   land windows hold under 10 m of relief, against 36.3% in Denmark.
#
# 0.00 IS REAL GROUND AND IS NOT MASKED, as in Schleswig-Holstein and unlike
#   Denmark. 28 of the 775 windows carry zeros, 27571 pixels in all, and the
#   ground that genuinely sits below the datum is written as a negative number
#   rather than as zero — the sampled minima run to -15.87 m. Nodata is
#   declared honestly as -9999 and needs no second mask.
#
# NOTHING LIES BELOW -20 m. The floor check that found Saarland's defect and
#   that a 1.6% sample got wrong in Denmark returns zero windows here, so there
#   is no spike tile to repair before rendering.
#
# THE DATUM IS SHARED WITH ENGLAND AND THEY WILL NOT AGREE. See the header of
#   download-wls.nu: this is built on OSTN15 and is right, England was built on
#   the Helmert fallback and is ~1.9 m wrong, so expect about 1.3 px of
#   disagreement at z16 along their border until England is rebuilt.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-wls.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:27700"                          # OSGB36 / British National Grid

gdal require-proj $EPSG "sample-zoom-wls.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "wls"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Wales"
    out_dir:   $"($DRIVE)/wls/zoom-sample"
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
    area_km2:  20780                               # Wales
    lat_min:   51.37
    lat_max:   53.43

    sites: [
        {name: "snowdon",    lat: 53.12129, lon: -4.09441,
         note: "Snowdon massif — rank 1, 590 m of relief in one km², slope p90 48.3°"}
        {name: "glyderau",   lat: 53.05747, lon: -4.05884,
         note: "Glyderau — rank 6, 437 m of relief with the floor already at 460 m"}
        {name: "cadair",     lat: 52.67281, lon: -3.83438,
         note: "Cadair Idris — rank 8, 391 m of relief"}
        {name: "rhondda",    lat: 51.59550, lon: -3.12086,
         note: "Rhondda valley sides — rank 4, 213 m, the South Wales coalfield"}
        {name: "margam",     lat: 51.50542, lon: -3.67831,
         note: "Margam opencast — rank 2, holds the -15.87 m floor at slope p50 3.7°"}
        {name: "pembroke",   lat: 51.69869, lon: -5.16161,
         note: "Pembrokeshire coast — rank 12, cliffs, 90.8% land in frame"}
        {name: "anglesey",   lat: 53.30856, lon: -4.20538,
         note: "Anglesey — rank 15, the flat end at slope p50 2.1°"}
    ]
}
