#!/usr/bin/env nu

# Fetch the Estonian DTM — the whole country at 1 m, in 4 km tiles over WCS,
# about 2840 land tiles and 45440 km2.
#
# Source: Maa- ja Ruumiamet (Estonian Land and Spatial Development Board).
#   https://teenus.maaamet.ee/ows/wcs-dtm  coverage `dtm-1`
#   Float32, 1 m, EPSG:3301 (L-EST97). The whole country is scanned, a quarter
#   of it re-flown every year; the current models are built from 2021-2024 ALS.
#
# LICENCE the Estonian open-data licence, which mirrors CC BY 4.0. Credit
#   "Maa- ja Ruumiamet", and the terms ask for the extraction date as well.
#
# WCS IS THE ONLY ROUTE, AND THAT IS NOT A PREFERENCE. The geoportal's 1 m
#   download is a session-bound search form with no static file URLs: the data
#   type and map sheet go in as `andmetyyp` and `kaardiruut`, and the links come
#   back server-side. There is no ATOM feed, no FTP, and the published WCS for
#   elevation is this one. So the fetch is the Flanders pattern — and as there,
#   KEEP THE CONSOLIDATED RESULT: re-fetching 45000 km2 over WCS is the
#   expensive part of any rebuild.
#
# MAXSIZE=4096 PER REQUEST, which is what sets the tile size. 4 km at 1 m is
#   4000 px a side, just inside it; 4096 would be the limit exactly and leaves
#   no margin. A 4 km tile takes about 3.1 s and 64 MB, a 2 km one 1.4 s and
#   16 MB — so the larger tile is better per unit area and is what this uses.
#
# THE QUEUE IS DRIVEN BY A LAND MASK, NOT BY THE SERVICE EXTENT. The coverage
#   bounding box is 375 x 260 km = 97500 km2 against 45339 km2 of land, so more
#   than half of it is sea. build-ee-mask.py renders the 25 m model nationally
#   (twelve 100 km windows, also MAXSIZE-bound) and marks every non-zero cell.
#   That mask measured 45440 km2 against the official 45339 — 0.2% out, which
#   is the check that the mask is right AND that 0.00 means sea.
#
# 0.00 IS THE SEA, AND THE AREA MATCH IS THE PROOF. Nodata is not declared
#   anywhere in this delivery, and a tile over open water comes back as a solid
#   block of zeros. Treating non-zero as land reproduces the national land area
#   to 0.2%, which it could not do if zero were ordinary ground.
#
#   THE MINES SURVIVE THAT READING. 5099 cells of the 25 m model sit below zero,
#   down to -13.58 m, about 3.2 km2 — the oil-shale open-casts in the north-east.
#   They are written as real negatives, not as zero, so masking zero leaves them
#   alone. Same shape as Denmark, and checked rather than assumed.
#
# Resumable: a tile already on disk with a readable raster is not refetched, and
# a tile is only marked done once it verifies. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-ee.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const WCS   = "https://teenus.maaamet.ee/ows/wcs-dtm"
const COV   = "dtm-1"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Estonia"
const EPSG  = "EPSG:3301"                            # L-EST97
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const TILE  = 4000                                   # m; 4000 px at 1 m, inside MAXSIZE 4096
const PAR   = 6
const QUEUE = "/home/martin/fm/freemap-outdoor-map/scripts/ee_tiles.txt"

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-ee.nu"

mkdir $DEST

if not ($QUEUE | path exists) {
    error make {msg: $"($QUEUE) not found — run build-ee-mask.py first; it writes the land-driven tile queue"}
}

let todo = (open --raw $QUEUE | lines | where {|l| ($l | str trim) != "" })
print $"==> ($todo | length) land tiles of ($TILE) m in the queue"

# ── Fetch ─────────────────────────────────────────────────────────────────────

mut n = 0
let total = ($todo | length)
mut i = 0

while $i < $total {
    let chunk = ($todo | skip $i | first $PAR)
    $i += $PAR

    $chunk | par-each -t $PAR {|line|
        # `split row` yields strings; the arithmetic below needs numbers.
        let p = ($line | split row " ")
        let x = ($p | get 0 | into int)
        let y = ($p | get 1 | into int)
        let out = $"($DEST)/ee_($x)_($y).tif"

        # A tile that opens is a tile we have; a truncated one does not.
        let have = (if ($out | path exists) {
            (do { ^gdalinfo $out } | complete).exit_code == 0
        } else { false })

        if not $have {
            rm -f $out
            let x1 = ($x + $TILE)
            let y1 = ($y + $TILE)
            let url = $"($WCS)?service=WCS&version=2.0.1&request=GetCoverage&coverageId=($COV)&format=image/tiff&subset=x\(($x),($x1)\)&subset=y\(($y),($y1)\)"
            (do { ^curl -sS -L --fail --max-time 600 -A $UA -o $"($out).raw" $url } | complete) | ignore

            # THE SERVICE SENDS UNCOMPRESSED TIFF — 64 MB a tile, 202 GB over the
            # country. DEFLATE with PREDICTOR=3 takes it to roughly a third.
            # PREDICTOR=3 is safe because this raster is only ever read by GDAL;
            # shading-ee.nu rewrites its window DEM with PREDICTOR=1 for
            # feature-preserving smoothing, which ignores the tag.
            #
            # The translate doubles as the integrity test: the service answers an
            # XML ExceptionReport with HTTP 200, so a failed request arrives as a
            # perfectly valid file that is not a raster.
            let c = (do {
                ^gdal_translate -q -of GTiff -co COMPRESS=DEFLATE -co PREDICTOR=3 -co ZLEVEL=6 -co TILED=YES $"($out).raw" $"($out).tmp"
            } | complete)
            rm -f $"($out).raw"

            if $c.exit_code == 0 and ($"($out).tmp" | path exists) {
                mv -f $"($out).tmp" $out
            } else {
                rm -f $"($out).tmp"
                print $"  FAILED ($x),($y) — left for the next run"
            }
        }
    } | ignore

    $n = (ls $"($DEST)/ee_*.tif" | length)
    if ($i mod 120) == 0 {
        print $"  ($n) / ($total) tiles present"
    }
}

# ── Verify and build the VRT ──────────────────────────────────────────────────

let have = (do { ^find $DEST -maxdepth 1 -name "ee_*.tif" } | complete | get stdout | lines)
print $"==> ($have | length) tiles present of ($total) queued"

if ($have | length) < $total {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
} else {
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # -srcnodata 0 masks the sea; see the header for why that is safe here.
        (gdal build-vrt $have $"($DEST)/all.vrt"
           --extra [-srcnodata 0 -vrtnodata -9999]
           --index $"($DEST)/tiles.txt")
    }
    print $"==> Done -> ($DEST)/all.vrt"
}
