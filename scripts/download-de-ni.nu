#!/usr/bin/env nu

# Fetch the Lower Saxon DGM1 (1 m digital terrain model), 1x1 km tiles.
#
# Source: LGLN (Landesamt für Geoinformation und Landesvermessung Niedersachsen)
#   via a STAC API. Cloud-Optimized GeoTIFF, EPSG:25832 (ETRS89 / UTM 32N),
#   heights DHHN2016 (EPSG:7837), Float32, LZW, nodata -9999, stated accuracy
#   0.3 m. Built by Delaunay triangulation from ALS point clouds of at least
#   4 points/m2 flown since 2019. Licence CC-BY-4.0, attribution LGLN.
#
# THE EASIEST SOURCE OF ANY STATE SO FAR, and the script is short because of it.
#   Saxony needed a 4981-entry tile table scraped out of an Angular form;
#   Thüringen needed an Atom feed, zip extraction, xyz-to-raster conversion and
#   two separate coordinate-convention repairs. Here the STAC API is paginated
#   and machine-readable, the assets are direct S3 URLs needing no auth, and a
#   COG is usable exactly as delivered. Nothing is converted.
#
# 70285 ITEMS COVER 49708 TILES — 41% OF TILES HAVE MORE THAN ONE VINTAGE, and
#   picking wrongly is the whole hazard here. Vintages run 2010-2025:
#
#     2025  7526    2021  5858    2017 14728    2013  5082
#     2023   690    2020  3705    2016 14368    2011    15
#     2022  1484    2019   905    2015 14240    2010   129
#     2018  1529    2014    22    2012     4
#
#   NEWEST WINS, per tile key. Downloading the catalogue as-is would fetch
#   246 GB and lay 20211 older tiles over newer ones in whatever order the VRT
#   happened to list them — a silent patchwork of surveys up to 15 years apart.
#   Taking the newest per key costs 174 GB and is deterministic.
#
#   Thüringen had the same trap at 1% scale and it still produced a half-pixel
#   seam. Here it is 41%. Do not "simplify" the grouping away.
#
# THE CATALOGUE IS PAGED ONCE AND CACHED to items.json, because walking 70285
#   items over ~140 requests is slow and the answer only changes when LGLN flies
#   again. Delete items.json to re-page.
#
# PARALLELISM: 24 measured clean against the S3 host (24/24 HTTP 200 in 1.3 s,
#   about 64 MB/s). This is object storage rather than a state portal, so it does
#   not throttle the way Saxony's Nextcloud share did.
#
# Resumable: a tile whose .tif already exists is skipped. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-ni.nu

const STAC  = "https://dgm.stac.lgln.niedersachsen.de/search"
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Niedersachsen"
const PAR   = 24
const TRIES = 4

mkdir $DEST
let cache = $"($DEST)/items.json"

if not ($cache | path exists) {
    print "==> Paging the STAC catalogue (once; delete items.json to refresh)"
    mut items = []
    mut body = {collections: ["dgm1"], limit: 500}
    mut page = 0
    loop {
        let d = (http post --content-type application/json $STAC $body)
        let feats = ($d | get -o features | default [])
        if ($feats | is-empty) { break }
        $items = ($items | append ($feats | each {|f|
            {
                id:   $f.id
                href: ($f | get -o assets.dgm1-tif.href)
                dt:   (($f | get -o properties.datetime | default "") | str substring 0..10)
            }
        }))
        $page = $page + 1
        if ($page mod 20) == 0 { print $"  ($items | length) items..." }
        let nxt = ($d | get -o links | default [] | where {|l| ($l | get -o rel) == "next"})
        if ($nxt | is-empty) { break }
        let nb = ($nxt | first | get -o body)
        if $nb == null { break }
        $body = $nb
    }
    print $"  ($items | length) items"
    $items | to json | save -f $cache
}

let items = (open $cache)
print $"==> ($items | length) catalogue items"

# Newest vintage per tile key. The id is dgm1_32_<E>_<N>_1_ni_<year>, so the
# key is the two coordinate fields; ties cannot happen because the same tile is
# never flown twice on one date.
let tiles = (
    $items
    | insert key {|r| ($r.id | split row "_" | slice 2..3 | str join "_") }
    | sort-by dt
    | group-by key
    | items {|k, v| $v | last }
)
print $"==> ($tiles | length) tiles after newest-per-key \(dropped (($items | length) - ($tiles | length)) older duplicates\)"

let todo = ($tiles | where {|t| not ($"($DEST)/($t.id).tif" | path exists) })
print $"==> ($todo | length) pending"

if ($todo | is-not-empty) {
    $todo | par-each -t $PAR {|t|
        let out = $"($DEST)/($t.id).tif"
        let tmp = $"($out).tmp"
        mut ok = false
        for attempt in 1..$TRIES {
            let r = (do { ^curl -sfL --max-time 300 -o $tmp $t.href } | complete)
            # Validate by opening it, not by size: a COG that downloads short is
            # still a file, and gdalbuildvrt would silently skip it later.
            if $r.exit_code == 0 and ($tmp | path exists) {
                let g = (do { ^gdalinfo $tmp } | complete)
                if $g.exit_code == 0 {
                    $ok = true
                    break
                }
            }
            sleep (($attempt * 3) | into duration --unit sec)
        }
        if $ok {
            mv $tmp $out
        } else {
            print $"  !! ($t.id): failed after ($TRIES) tries"
            rm -f $tmp
        }
    }
}

let have = (glob $"($DEST)/*.tif" | length)
print $"==> ($have) of ($tiles | length) tiles present"

if $have < ($tiles | length) {
    print "    Re-run to retry the missing ones; finished tiles are skipped."
} else {
    print "==> Building all.vrt"
    let idx = $"($DEST)/tiles.txt"
    glob $"($DEST)/*.tif" | str join "\n" | save -f $idx
    gdalbuildvrt -input_file_list $idx $"($DEST)/all.vrt" o> /dev/null
    rm -f $idx
    print $"==> Done -> ($DEST)/all.vrt"
}
