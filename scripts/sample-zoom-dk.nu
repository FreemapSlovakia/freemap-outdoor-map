#!/usr/bin/env nu

# Sample Denmark at three zooms, across seven windows, so the ZOOM for
# shading-dk.nu is chosen from evidence.
#
# THE SITES COME FROM A RANDOM SAMPLE, NOT A NATIONAL RANKING. Scanning all
#   50013 rasters takes hours off this drive, and the zoom decision does not
#   need the single roughest square kilometre in the country — it needs a
#   spread of terrain that is representative and includes the demanding end.
#   800 rasters were scanned at random and the windows drawn from their
#   ranking, so a rank quoted below is a rank within that sample.
#
#     Himmerland                 rank   1   0.070   of tiles under 5% sea
#     Rold Skov edge             rank   4   0.052   31.5 m, the most inland relief
#     Stevns Klint               coastal 0.101   chalk cliff, 80% land in frame
#     Bornholm                   coastal 0.089   granite, the only hard rock
#     Sjælland moraine           rank   2   0.059   ordinary hill country
#     West Jutland dune coast    rank   3   0.054   blown sand at 2.5 m
#     Lammefjord                 flat            the lowest land, -7.65 m
#
#   THE RANKING IS TAKEN OVER TILES UNDER 5% SEA, NOT OVER ALL OF THEM. In a
#   country this flat the roughest ground is the coast, so an unfiltered
#   ranking picks tiles that are half water and puts the sample window in the
#   sea: five of the first seven sites chosen that way had their centre pixel
#   masked, one with only 11% land in frame. Stevns and Bornholm are carried
#   from the coastal set deliberately, moved to the centroid of their land.
#
#   Denmark is flat, so the national roughness median is 0.025 against
#   Schleswig-Holstein's 0.036 -- the decision is made on ground that has less
#   to resolve than anywhere already rendered, and Lammefjord is carried to
#   show what the flat end actually looks like at each zoom.
#
# 0.00 IS THE SEA AND IS MASKED by -srcnodata 0 in all.vrt, which is the
#   Mecklenburg-Vorpommern decision and the opposite of Schleswig-Holstein's.
#   It was measured, not assumed: of 125 million zero pixels in the sample only
#   0.176% touch a non-zero neighbour, so they are a few enormous blobs rather
#   than scattered ground, and exact 0.00 outnumbers the two 0.25 m bands on
#   either side of it by 23 times. Real terrain crossing the datum would be
#   smooth across zero, not spiked at it.
#
#   THE LAND BELOW SEA LEVEL IS SAFE FROM THAT MASK because it is not written
#   as zero: Lammefjord runs -7.65 m to -1.41 m across a whole tile, and the
#   polders generally sit between -1 m and -4 m. Masking 0.00 costs them
#   nothing.
#
# -9999 IS WRITTEN, AND ONLY AS WHOLE TILES. 28 rasters are entirely -9999;
#   they were rewritten to 0.00 by the downloader, because with -srcnodata 0
#   masking the sea a surviving -9999 would stop being nodata and become
#   terrain 9999 m down, which is the Hamburg trap and is not noticed until
#   gdal_contour refuses "too many levels" an hour later.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-dk.nu

use lib/gdal.nu
use lib/zoomsample.nu

const EPSG = "EPSG:25832"                          # ETRS89 / UTM 32N

gdal require-proj $EPSG "sample-zoom-dk.nu"

let DRIVE = (gdal find-drive)

zoomsample run {
    code:      "dk"
    src_dir:   "/run/media/martin/2190983A5767510F/DGM1/Denmark"
    out_dir:   $"($DRIVE)/dk/zoom-sample"
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
    area_km2:  42933                               # Denmark, land area
    lat_min:   54.56
    lat_max:   57.75

    sites: [
        {name: "himmerland",  lat: 56.91840, lon: 9.66524,
         note: "Himmerland — roughest inland tile in the sample, 22.2 m of relief"}
        {name: "rold",        lat: 56.94415, lon: 9.86295,
         note: "Rold Skov edge — the most relief of any inland tile, 31.5 m"}
        {name: "stevns",      lat: 55.31791, lon: 12.43736,
         note: "Stevns Klint — chalk cliff and quarry, 80% land in frame"}
        {name: "bornholm",    lat: 55.04097, lon: 14.86732,
         note: "Bornholm — granite, the only hard rock in the country"}
        {name: "sjaelland",   lat: 55.41627, lon: 12.10458,
         note: "Sjælland moraine — ordinary Danish hill country"}
        {name: "vestjylland", lat: 55.43603, lon: 8.35991,
         note: "West Jutland dune coast — blown sand at 2.5 m"}
        {name: "lammefjord",  lat: 55.77980, lon: 11.54328,
         note: "Lammefjord polder — the flat end, Denmark's lowest land"}
    ]
}
