#!/usr/bin/env nu

# Fetch the Mecklenburg-Vorpommern DGM1 (1 m digital terrain model) — 6407
# tiles of 2x2 km, about 49 GB once recompressed.
#
# Source: Landesamt für innere Verwaltung M-V, INSPIRE ATOM download service.
#   https://www.geodaten-mv.de/dienste/dgm_atom
#   GeoTIFF, Float32, 1 m, EPSG:25833.
#
# LICENCE CC BY 4.0, with the credit prescribed verbatim:
#   "© GeoBasis-DE/M-V <Jahr der letzten Datenlieferung>"
#
# EPSG:25833, like Brandenburg and unlike every other German state here, but a
#   plain PROJCRS rather than Brandenburg's mixed COMPOUNDCRS — uniform over
#   every tile sampled, so no stamping step.
#
# SIX PRODUCTS ARE OFFERED AND ONLY ONE IS ELEVATION. The dataset feed carries
#   six entries of 6407 tiles each, distinguished only by a word in the title:
#
#     (ZCoding mit Shading)  _mix.tif       a picture
#     (XYZ)                  _xyz.zip       ASCII triples
#     (Shading)              _schum_NW.tif  a hillshade, already rendered
#     (Zcoding)              _zcode.tif     height encoded into RGB
#     (Gtiff)                _gtiff.tif     THE ONE WE WANT — Float32 metres
#     (Isoli)                _isoli.zip     ready-made isolines
#
#   Pick by the "(Gtiff)" title, not by position: they are all image/tiff and
#   all 6407 sections, so a wrong pick downloads 100 GB of shaded pictures and
#   only fails much later, at the contour stage.
#
# THE TILES ARRIVE UNCOMPRESSED — 16 MB each, 102 GB for the state. They are
#   recompressed to DEFLATE on the way in: measured on one tile, 15.2 MB raw ->
#   10.3 MB LZW -> 7.7 MB DEFLATE, both with PREDICTOR=3, at about 1.3 s per
#   tile. PREDICTOR=3 is safe here because this raster is only ever read by
#   GDAL; shading-de-mv.nu rewrites its window DEM with PREDICTOR=1 for
#   feature-preserving-smoothing, which ignores the tag.
#
# 0.00 IS THE BALTIC, AND IT IS MASKED. No nodata is declared anywhere in the
#   delivery, and the sea is written as a flat 0.00 surface: the four
#   northernmost tiles measured 68%, 79%, 95% and 68% exact zeros. `-srcnodata
#   0` on the VRT turns that into a void, which is what Niedersachsen does with
#   its own open water — a shaded flat sea is worse than no shading, and the
#   renderer draws water over it either way.
#
#   LAND BORDERS ARE NOT PADDED, so this costs nothing inland: the four
#   southernmost tiles, on the Brandenburg boundary, hold real terrain from
#   11.7 m to 26.6 m and not one zero between them. The only inland loss is
#   ground at exactly 0.00 m, which is the waterline.
#
#   Inland lakes are unaffected — they carry their real surface height, the
#   Müritz at about 62 m, so they stay flat interpolated water as in
#   Brandenburg rather than becoming holes.
#
# THE DATASET ID IS NOT HEX and looks corrupt — ca268792-s2q1-4a39-b34c-
#   9ec5bf9a4469 contains s and q. It is resolved from the service feed each
#   run rather than pinned, so a reissue does not silently 400.
#
# Resumable: a tile already converted is not refetched, so a re-run costs one
# directory listing. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-mv.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const FEED  = "https://www.geodaten-mv.de/dienste/dgm_atom"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Mecklenburg-Vorpommern"
const STAGE = "/mnt/osm/mv-stage"                    # raw tiles + aria2 log, on NVMe
const EPSG  = "EPSG:25833"                           # ETRS89 / UTM zone 33N
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"

# 16 MB each and the server gave 28 MB/s across 7 parallel fetches, so a
# handful of connections saturates it; more would only deepen the buffer.
const DL_PAR = 8

# Recompressors draining the buffer. 1.3 s per tile, so 8 keeps well ahead of
# the link and the staging directory stays small.
const PAR = 8

const EXPECTED = 6407

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-mv.nu"

mkdir $DEST
mkdir $STAGE

# ── Resolve the DGM1 dataset feed, then the Gtiff entry ───────────────────────

print "==> fetching the service feed"
let svc = $"($STAGE)/service.xml"
(do { ^curl -sS -L --fail --max-time 120 -A $UA -o $svc $FEED } | complete) | ignore
if not ($svc | path exists) {
    error make {msg: $"could not fetch ($FEED)"}
}

# The DGM1, DGM5 and DGM25 entries differ only by title.
let dgm1 = (
    (open --raw $svc | split row "<entry>")
      | skip 1
      | where {|e| $e | str contains "DGM1" }
      | first
)
let ds_url = (
    $dgm1 | parse -r 'rel="alternate"[^>]*?href="([^"]+)"'
          | get capture0 | first | str replace --all "&amp;" "&"
)
print $"==> dataset feed: ($ds_url)"

print "==> fetching the DGM1 dataset feed \(about 15 MB\)"
let dsf = $"($STAGE)/dgm1.xml"
(do { ^curl -sS -L --fail --max-time 600 -A $UA -o $dsf $ds_url } | complete) | ignore
if not ($dsf | path exists) {
    error make {msg: $"could not fetch ($ds_url)"}
}

# SELECT BY TITLE, NOT POSITION — see the header. Six entries, one elevation.
let gtiff = (
    (open --raw $dsf | split row "<entry>")
      | skip 1
      | where {|e| $e | str contains "(Gtiff)" }
      | first
)

let urls = (
    $gtiff | parse -r '<link rel="section"[^>]*?href="([^"]+)"'
           | get capture0
           | each {|u| $u | str replace --all "&amp;" "&" }
           | where {|u| $u | str ends-with "_gtiff.tif" }
)
print $"==> ($urls | length) tiles offered \(expected ($EXPECTED)\)"
if ($urls | length) < 100 {
    error make {msg: "the Gtiff entry parsed to almost nothing — the feed format has changed"}
}
if ($urls | length) != $EXPECTED {
    print $"    NOTE: differs from the ($EXPECTED) seen on 2026-09-27 — coverage may have grown"
}

# ── Work out what is missing ──────────────────────────────────────────────────

print "==> working out what is missing"
let present = (
    do { ^find $DEST -maxdepth 1 -name "*.tif" -printf "%f\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|f, acc| $acc | upsert $f true }
)

let todo = (
    $urls
      | each {|u| {url: $u, name: ($u | split row "file=" | last)} }
      | where {|t| not ($present | get -o $t.name | default false) }
)
print $"==> ($todo | length) to fetch, ($urls | length) total"

# ── Download, recompressing as it goes ────────────────────────────────────────

if ($todo | is-not-empty) {
    let raw = $"($STAGE)/raw"
    mkdir $raw
    rm -f $"($raw)/.dl-done"

    # Below the 16000000 bytes of pixel data a tile cannot be complete, so it
    # is a truncated leftover and is simply fetched again.
    for f in (glob $"($raw)/*.tif") {
        if not ($"($f).aria2" | path exists) {
            if (ls $f | get 0.size | into int) < 16000000 { rm -f $f }
        }
    }

    ($todo | each {|t| $"($t.url)\n  out=($t.name)" } | str join "\n")
      | save -f $"($raw)/urls.txt"

    # The fetch goes in a script file so `&` has one command to background;
    # `sh -c "aria2c ...; touch done &"` backgrounds only the touch and leaves
    # the drain loop below waiting for the whole download to finish.
    [
        "#!/bin/sh"
        $"aria2c -i '($raw)/urls.txt' -j ($DL_PAR) -x1 -s1 --continue=true --auto-file-renaming=false --user-agent '($UA)' --connect-timeout=20 --timeout=120 --max-tries=5 --retry-wait=5 --console-log-level=error --summary-interval=0 -d '($raw)' > '($raw)/aria2.log' 2>&1"
        $"touch '($raw)/.dl-done'"
    ] | str join "\n" | save -f $"($raw)/fetch.sh"

    (do { ^bash -c $"nohup sh '($raw)/fetch.sh' > /dev/null 2>&1 &" } | complete) | ignore
    print $"==> downloader started \(($DL_PAR) concurrent\), recompressing with ($PAR) workers"

    mut n = 0
    loop {
        # SIZE, NOT JUST THE CONTROL FILE. aria2c creates the .tif before its
        # .aria2 sibling, so "no control file" also matches a tile that has
        # barely started: measured on the first run, 1776 of 2007 drain
        # attempts were gdal_translate against a truncated file.
        #
        # THE THRESHOLD IS THE PIXEL DATA, NOT A FIXED FILE SIZE. 2000x2000
        # Float32 uncompressed is exactly 16000000 bytes and the TIFF header
        # varies by a few KB on top — 16024498 for most tiles but 16016507 for
        # some. Testing equality against one observed size rejects the others
        # forever: they download, fail the test, get deleted as truncated and
        # refetched, and the run can never finish. Partial files are orders of
        # magnitude smaller, so a floor separates them cleanly.
        let ready = (
            glob $"($raw)/*.tif"
              | where {|f| not ($"($f).aria2" | path exists) }
              | where {|f| (ls $f | get 0.size | into int) >= 16000000 }
        )

        if ($ready | is-empty) {
            if ($"($raw)/.dl-done" | path exists) { break }
            sleep 3sec
            continue
        }

        $ready | par-each -t $PAR {|f|
            let name = ($f | path basename)
            let out = $"($DEST)/($name)"

            if not ($out | path exists) {
                let c = (do {
                    ^gdal_translate -q -of GTiff -co COMPRESS=DEFLATE -co PREDICTOR=3 -co ZLEVEL=6 -co TILED=YES -co NUM_THREADS=2 $f $"($out).tmp"
                } | complete)
                if $c.exit_code == 0 and ($"($out).tmp" | path exists) {
                    mv -f $"($out).tmp" $out
                } else {
                    rm -f $"($out).tmp"
                    print $"  FAILED recompress ($name)"
                    return
                }
            }
            rm -f $f
        } | ignore

        $n += ($ready | length)
        print $"  converted ($n) / ($todo | length)"
    }
}

# ── Verify and build the state VRT ────────────────────────────────────────────

let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
let missing = ($urls | each {|u| $u | split row "file=" | last } | where {|n| not ($"($DEST)/($n)" | path exists) })
print $"==> ($have | length) tiles present; ($missing | length) missing"

if ($missing | is-not-empty) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    first few: ($missing | first 5 | str join ', ')"
} else {
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # -srcnodata 0 masks the Baltic — see the header. It costs no land
        # because the delivery pads no border with zeros.
        (gdal build-vrt $have $"($DEST)/all.vrt"
           --extra [-srcnodata 0 -vrtnodata -9999]
           --index $"($DEST)/tiles.txt")
    }
    rm -rf $STAGE
    print $"==> Done -> ($DEST)/all.vrt"
}
