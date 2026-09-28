#!/usr/bin/env nu

# Fetch the Bremen DGM1 (1 m digital terrain model) — 583 tiles of 1x1 km in
# two archives, about 1 GB once converted.
#
# Source: Landesamt GeoInformation Bremen, INSPIRE download service.
#   https://gdi2.geo.bremen.de/inspire/download/DGM/data/
#   XYZ ASCII, 1 m, EPSG:25832.
#
# LICENCE CC BY 4.0, credit "Landesamt GeoInformation Bremen". Open since
#   2024-06-09.
#
# THE STATE IS TWO CITIES 60 km APART AND TWO SURVEYS TWO YEARS APART:
#
#     Gitternetz_DGM1_2015_BHV_ASCII_XYZ.zip   188 tiles   Bremerhaven, 2015
#     Gitternetz_DGM1_2017_HB_ASCII_XYZ.zip    395 tiles   Bremen, 2017
#
#   and the two disagree about how to write a coordinate. Each needs its own
#   repair; a single rule applied to both puts one of them in the wrong place.
#
# BREMERHAVEN'S EASTINGS CARRY THE UTM ZONE. Its first column reads
#   32467000.5, not 467000.5 — the zone number is prefixed onto the easting, an
#   old AdV convention. Fed to GDAL unchanged, every Bremerhaven tile lands at
#   easting 32 467 000, about 2900 km east of Bremen and past the Urals, and
#   the VRT spans a continent without complaining. Subtract 32000000.
#
# BREMEN'S COORDINATES ARE CELL CORNERS, NOT CENTRES. Its first column reads
#   474000 where Bremerhaven reads 32467000.5, and the XYZ driver assumes every
#   coordinate is a cell centre. Taken at face value the tile would cover
#   473999.5 to 474999.5 — half a metre off the kilometre grid, and half a
#   metre out of step with Bremerhaven, which is the Rheinland-Pfalz trap
#   except between two halves of one state. Add 0.5 to x and y; the tile then
#   covers 474000 to 475000 as its name says.
#
#   Verified after repair: Bremerhaven origin 467000/5927000, Bremen origin
#   474000/5892000, both exactly on the km grid.
#
# BOTH ARCHIVES CARRY AN "x y z" HEADER LINE, which gdal_translate rejects, so
#   it is dropped along with the coordinate fix in one awk pass.
#
# NODATA: none is declared and none is written — every tile is a full million
#   points. 0.00 is real ground: Bremen sits on the Weser marsh and values run
#   below sea level. Do not mask it. The VRT gets -vrtnodata so that ground no
#   tile covers is excluded rather than read as 0.00, which is the
#   Schleswig-Holstein lesson.
#
# Resumable: a tile already converted is not redone. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-hb.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const BASE  = "https://gdi2.geo.bremen.de/inspire/download/DGM/data"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Bremen"
const STAGE = "/mnt/osm/hb-stage"                    # archives + extraction, on NVMe
const EPSG  = "EPSG:25832"                           # ETRS89 / UTM zone 32N
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const PAR   = 8
const EXPECTED = 583

# Each archive with the awk expression that puts its coordinates on the km grid
# — see the header. dx is applied to x, dy to y.
const PARTS = [
    {file: "Gitternetz_DGM1_2015_BHV_ASCII_XYZ.zip", local: "bhv_2015.zip", dx: -32000000, dy: 0}
    {file: "Gitternetz_DGM1_2017_HB_ASCII_XYZ.zip",  local: "hb_2017.zip",  dx: 0.5,       dy: 0.5}
]

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-hb.nu"

mkdir $DEST
mkdir $STAGE

let work = $"($STAGE)/x"
mkdir $work

# ── Fetch, repair, convert ────────────────────────────────────────────────────

for part in $PARTS {
    let zip = $"($STAGE)/($part.local)"

    if not ($zip | path exists) {
        print $"==> fetching ($part.file)"
        (do { ^curl -sS -L --fail --max-time 3600 -A $UA -o $zip $"($BASE)/($part.file)" } | complete) | ignore
        if not ($zip | path exists) {
            error make {msg: $"could not fetch ($BASE)/($part.file)"}
        }
    }

    if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
        rm -f $zip
        error make {msg: $"($part.local) failed its CRC check and was removed — re-run to refetch"}
    }

    let names = (
        do { ^unzip -Z1 $zip } | complete | get stdout | lines
          | where {|n| $n | str ends-with ".xyz" }
    )
    let todo = ($names | where {|n| not ($"($DEST)/(($n | path basename) | str replace '.xyz' '.tif')" | path exists) })
    print $"==> ($part.local): ($names | length) tiles, ($todo | length) to convert  \(dx ($part.dx), dy ($part.dy)\)"

    if ($todo | is-not-empty) {
        $todo | par-each -t $PAR {|n|
            let base = ($n | path basename)
            let stem = ($base | str replace ".xyz" "")
            let raw = $"($work)/($base)"
            let fixed = $"($work)/($stem).fix.xyz"
            let out = $"($DEST)/($stem).tif"

            (do { ^unzip -joqq $zip $n -d $work } | complete) | ignore
            if not ($raw | path exists) {
                print $"  MISSING after extract: ($base)"
                return
            }

            # One pass: drop the "x y z" header and move the coordinates onto
            # the kilometre grid. %.1f because the shift can leave a half metre.
            (do {
                ^bash -c $"tail -n +2 '($raw)' | awk '{printf \"%.1f %.1f %s\\n\", $1 + ($part.dx), $2 + ($part.dy), $3}' > '($fixed)'"
            } | complete) | ignore
            rm -f $raw

            let c = (do {
                ^gdal_translate -q -of GTiff -ot Float32 -a_srs $EPSG -co COMPRESS=DEFLATE -co PREDICTOR=3 -co ZLEVEL=6 -co TILED=YES $fixed $"($out).tmp"
            } | complete)
            rm -f $fixed

            if $c.exit_code == 0 and ($"($out).tmp" | path exists) {
                mv -f $"($out).tmp" $out
            } else {
                rm -f $"($out).tmp"
                print $"  FAILED convert ($stem)"
            }
        } | ignore
    }
}

# ── Verify the grid, then build the VRT ───────────────────────────────────────

let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
print $"==> ($have | length) rasters present \(expected ($EXPECTED)\)"

if ($have | length) < $EXPECTED {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
} else {
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # -vrtnodata only: nothing in the sources is nodata, and 0.00 is real
        # marsh, so this fills ground no tile covers and nothing else.
        (gdal build-vrt $have $"($DEST)/all.vrt"
           --extra [-vrtnodata -9999]
           --index $"($DEST)/tiles.txt")
    }
    rm -rf $work
    print $"==> Done -> ($DEST)/all.vrt"
}
