#!/usr/bin/env nu

# Fetch the Schleswig-Holstein DGM1 (1 m digital terrain model) — 18685 tiles
# of 1x1 km, about 32 GB once converted.
#
# Source: LVermGeo Schleswig-Holstein, OpenGBD download portal.
#   https://geodaten.schleswig-holstein.de/gaialight-sh/_apps/dladownload/dl-dgm1.html
#   XYZ ASCII, 1 m, EPSG:25832.
#
# LICENCE CC BY 4.0. Credit "© GeoBasis-DE/LVermGeo SH, CC BY 4.0".
#
# THE INDEX IS A GeoJSON OF EVERY TILE, which is the best thing about this
#   portal: one 9 MB document naming all 18685 tiles, each with its download
#   link and — uniquely among the German states here — its survey date. No
#   scraping and no tile index to encode.
#
# 523 GB ON THE WIRE FOR 32 GB OF DATA. The delivery is uncompressed ASCII, 28
#   MB a tile, and the server does not honour Accept-Encoding: gzip (measured;
#   gzip -6 would take a tile to 3.7 MB). Nothing can be done about that, so
#   the raw text is never stored: each tile is fetched to tmpfs, converted, and
#   deleted. Measured 40 MB/s across 7 parallel fetches, so the whole state is
#   about four hours.
#
# EVERY DOWNLOAD HAS AN HTML PAGE STAPLED TO ITS END. massen.php streams the
#   1000000 data lines, then a blank line, then 31 lines of markup for a
#   "Zurück zum OpenGBD-Downloadportal" button. gdal_translate stops at
#   "At line 1000002, found 2 tokens. Expected 3 at least" and writes nothing,
#   so the lines are filtered to those starting with a digit before conversion.
#   The head of the file is clean; only the tail is contaminated.
#
# THE VINTAGES ARE SPLIT TWENTY YEARS APART. 4542 tiles are 2005-2007 and the
#   rest 2020-2025, with almost nothing between:
#
#     2005-2007   4542 tiles
#     2011-2019     30
#     2020-2025  14113
#
#   So a quarter of the state is first-generation laser scan and will look
#   coarser than the rest. Expect the roughness scan to rank those areas low
#   for reasons of survey age rather than terrain, and check the sample sites'
#   dates before drawing conclusions from their scores.
#
# CELL-CENTRE REGISTERED, AND GDAL WORKS IT OUT. The points are at x.50, y.50,
#   and the XYZ driver infers the corner origin correctly — a tile starting at
#   424000.50 lands at exactly 424000. No regridding is needed, unlike
#   Rheinland-Pfalz's 78 stray tiles.
#
# NO NODATA IS DECLARED and negative heights are real: the first tile sampled
#   runs -2.44 m to -0.23 m, which is tidal flat, not a sentinel. Verify what
#   0.00 means over the whole state before rendering — Mecklenburg-Vorpommern
#   writes its sea as a flat 0.00 and Baden-Württemberg uses 0.00 for
#   out-of-coverage, while Brandenburg's zeros are real ground.
#
# Resumable: a tile already converted is not refetched. The output name carries
# the survey year, which the index supplies, so the check is an exact name and
# not a glob as in Baden-Württemberg. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-sh.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const INDEX = "https://geodaten.schleswig-holstein.de/gaialight-sh/_apps/dladownload/single.php?file=DGM1_SH__Massendownload.geojson&id=4"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Schleswig-Holstein"
const STAGE = "/mnt/osm/sh-stage"                    # index only; tiles go to tmpfs
const WORK  = "/dev/shm/sh_work"                     # 28 MB a tile, never on disk
const EPSG  = "EPSG:25832"                           # ETRS89 / UTM zone 32N
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"

# Each worker fetches a tile then converts it, rather than the aria2c-plus-drain
# split the zip-based states use: here the download is the slower half (28 MB at
# roughly 6 MB/s a connection against 1.5 s to convert), so there is no buffer
# to keep full and no partial-file race to guard against. 12 saturates the
# measured 40 MB/s.
const PAR = 12

const EXPECTED = 18685

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-sh.nu"

mkdir $DEST
mkdir $"($DEST)/.unavailable"
mkdir $STAGE
mkdir $WORK

# ── Fetch and parse the index ─────────────────────────────────────────────────

let idx = $"($STAGE)/massendownload.geojson"
print "==> fetching the tile index \(about 9 MB\)"
(do { ^curl -sS -L --fail --max-time 300 -A $UA -o $idx $INDEX } | complete) | ignore
if not ($idx | path exists) {
    error make {msg: $"could not fetch ($INDEX)"}
}

let tiles = (
    open --raw $idx
      | parse -r '"datum":\s*"(?<datum>[^"]*)",\s*"link_data":\s*"(?<url>[^"]*)"'
      | each {|r| {
            url: ($r.url | str replace --all "&amp;" "&"),
            name: ($r.url | parse -r 'file=([^&]+)' | get capture0.0),
            datum: $r.datum,
        } }
      | each {|r| $r | insert stem ($r.name | str replace ".xyz" "") }
)
print $"==> index lists ($tiles | length) tiles \(expected ($EXPECTED)\)"
if ($tiles | length) < 1000 {
    error make {msg: "the index parsed to almost nothing — the GeoJSON format has changed"}
}
if ($tiles | length) != $EXPECTED {
    print $"    NOTE: differs from the ($EXPECTED) seen on 2026-09-28 — coverage may have grown"
}

# ── Work out what is missing ──────────────────────────────────────────────────

print "==> working out what is missing"
let present = (
    do { ^find $DEST -maxdepth 1 -name "*.tif" -printf "%f\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|f, acc| $acc | upsert $f true }
)

let unavailable = (
    do { ^find $"($DEST)/.unavailable" -maxdepth 1 -type f -printf "%f\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|f, acc| $acc | upsert $f true }
)

let todo = (
    $tiles | where {|t| not ($present | get -o $"($t.stem).tif" | default false) }
           | where {|t| not ($unavailable | get -o $t.stem | default false) }
)
print $"==> ($todo | length) to fetch, ($tiles | length) total, ($unavailable | columns | length) known unavailable"

# ── Fetch, strip, convert ─────────────────────────────────────────────────────

if ($todo | is-not-empty) {
    print $"==> ($PAR) workers; raw XYZ staged in ($WORK) and deleted as it goes"

    $todo | par-each -t $PAR {|t|
        let raw = $"($WORK)/($t.stem).raw"
        let xyz = $"($WORK)/($t.stem).xyz"
        let out = $"($DEST)/($t.stem).tif"

        let dl = (do { ^curl -sS -L --fail --max-time 600 -A $UA -o $raw $t.url } | complete)
        if $dl.exit_code != 0 or not ($raw | path exists) {
            print $"  FAILED download ($t.stem)"
            rm -f $raw
            return
        }

        # SOME TILES ARE LISTED BUT NOT SERVED. massen.php answers 200 with a
        # 962-byte page saying "konnte nicht heruntergeladen werden ... die
        # verwendete Massendownload-Datei ist veraltet", although the index was
        # fetched minutes earlier and the portal offers no other. Measured over
        # the first 412 tiles: 56 of them, every one in the Wadden Sea, the
        # North Frisian islands or off Helgoland, and no survey year works for
        # them. They are marked so a re-run neither refetches them nor leaves
        # `missing` permanently non-empty, which would mean the VRT is never
        # built.
        if (($raw | path exists) and ((ls $raw | get 0.size | into int) < 100000)) {
            let body = (open --raw $raw)
            if ($body | str contains "konnte nicht heruntergeladen werden") {
                touch $"($DEST)/.unavailable/($t.stem)"
                rm -f $raw
                return
            }
        }

        # Drop the HTML footer massen.php appends — see the header.
        (do { ^bash -c $"awk '/^[0-9]/' '($raw)' > '($xyz)'" } | complete) | ignore
        rm -f $raw

        # DO NOT REQUIRE A FULL MILLION POINTS. A kilometre square at 1 m holds
        # 1000000, but the state's edge tiles are clipped and hold fewer —
        # measured 426 of them, at 999000, 800000, 600000 and other counts,
        # every one converting cleanly to a 1000x999, 800x1000 or 1000x600
        # raster with the right origin. Rejecting them on the count threw away
        # the entire coast and both land borders. An empty file is the only
        # thing worth refusing here; whether the rest forms a usable grid is
        # gdal_translate's judgement, not ours.
        let n = (do { ^bash -c $"wc -l < '($xyz)'" } | complete | get stdout | str trim | into int)
        if $n == 0 {
            print $"  EMPTY ($t.stem) — no data lines"
            rm -f $xyz
            return
        }

        # -ot Float32 IS LOAD-BEARING. The XYZ driver picks the data type from
        # the values it reads, so a tile whose heights are all whole numbers
        # comes out as an integer band — and PREDICTOR=3 then fails outright
        # with "only supported with Float16, Float32 or Float64", writing
        # nothing. Exactly one tile in 18685 does this, the all-zero sea square
        # at E 617000 N 5985000 in Lübeck Bay, so without the flag the state is
        # one tile short forever and the VRT is never built. Forcing the type
        # also keeps every tile the same, which the VRT wants anyway.
        #
        # PREDICTOR=3 is safe: this raster is read by GDAL only. shading-de-sh.nu
        # rewrites its window DEM with PREDICTOR=1 for feature-preserving
        # smoothing, which ignores the tag.
        let c = (do {
            ^gdal_translate -q -of GTiff -ot Float32 -a_srs $EPSG -co COMPRESS=DEFLATE -co PREDICTOR=3 -co ZLEVEL=6 -co TILED=YES $xyz $"($out).tmp"
        } | complete)
        rm -f $xyz

        if $c.exit_code == 0 and ($"($out).tmp" | path exists) {
            mv -f $"($out).tmp" $out
        } else {
            rm -f $"($out).tmp"
            print $"  FAILED convert ($t.stem)"
        }
    } | ignore
}

# ── Verify and build the state VRT ────────────────────────────────────────────

let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
let gone = (
    do { ^find $"($DEST)/.unavailable" -maxdepth 1 -type f -printf "%f\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|f, acc| $acc | upsert $f true }
)
let missing = (
    $tiles | where {|t| not ($"($DEST)/($t.stem).tif" | path exists) }
           | where {|t| not ($gone | get -o $t.stem | default false) }
)
print $"==> ($have | length) tiles present; ($missing | length) missing; ($gone | columns | length) not served by the portal"

if ($missing | is-not-empty) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    first few: ($missing | first 5 | get stem | str join ', ')"
} else {
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # -vrtnodata IS NOT A SENTINEL MASK, IT IS THE FILL FOR UNCOVERED GROUND.
        # The delivery declares no nodata and 0.00 is real marsh here, so there
        # is deliberately no -srcnodata; but without -vrtnodata everything
        # outside the state reads as 0.00 too, which is the same value as the
        # Wilstermarsch. The shading would then see terrain at sea level across
        # Denmark, Hamburg and the North Sea instead of nothing. -9999 appears
        # nowhere in the sources (the state minimum is about -3.7 m), so it can
        # only ever mean "no tile here".
        (gdal build-vrt $have $"($DEST)/all.vrt"
           --extra [-vrtnodata -9999]
           --index $"($DEST)/tiles.txt")
    }
    rm -rf $STAGE
    rm -rf $WORK
    print $"==> Done -> ($DEST)/all.vrt"
}
