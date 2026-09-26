#!/usr/bin/env nu

# Fetch the Brandenburg DGM1 (1 m digital terrain model) — 31291 tiles of
# 1x1 km, about 41 GB.
#
# Source: Landesvermessung und Geobasisinformation Brandenburg (LGB), open data.
#   https://data.geobasis-bb.de/geobasis/daten/dgm/
#   GeoTIFF, Float32, LZW, nodata -9999, 1 m.
#
# LICENCE dl-de/by-2-0, credit "© GeoBasis-DE/LGB", and "(Daten geändert)"
#   appended once the data is resampled, as the shading does.
#
# BERLIN COMES WITH IT. The grid is continuous across the enclave — 961 of the
#   961 tiles in the Berlin block are offered — so this one download covers two
#   federal states. Whether Berlin's tiles carry real data or nodata filler is
#   checked below before the VRT is built.
#
# EPSG:25833, THE FIRST ZONE-33 STATE HERE. Every German state so far has been
#   UTM 32N; Brandenburg is 33N, so nothing that hardcodes 25832 can be reused
#   without reading it.
#
# THE CRS IS MIXED, AND THIS IS THE WORST SPLIT YET: measured over 2000 rasters,
#   70% carry COMPOUNDCRS "ETRS89 / UTM 33N + DHHN2016 height" and 30% a plain
#   PROJCRS, so about 9400 tiles differ from the rest. gdalbuildvrt drops
#   whichever group is not the first input, and its complaint names the two as
#   identical — "expected ETRS89 / UTM zone 33N, got ETRS89 / UTM zone 33N" with
#   only the vertical part differing — behind its progress bar. Rheinland-Pfalz
#   lost 78 tiles that way; here it would be a third of the state. stamp_crs.py
#   normalises onto the majority below and `gdal build-vrt` verifies every
#   offered input back out of the VRT.
#
#   Read the split with GetSpatialRef().IsCompound(), not GetProjection(): the
#   latter returns only the horizontal part, so every raster looks like a plain
#   PROJCRS and the mix is invisible.
#
# THE INDEX IS THE LISTING PAGE, not a hardcoded table. It is one 10.9 MB HTML
#   document naming every archive, so unlike Baden-Württemberg there is no tile
#   index to run-length encode and no portal to reverse-engineer. Fetched fresh
#   each run; coverage can only grow.
#
#   IT HANGS OFF /tif/, NOT /dgm/. The parent path serves a 6.8 KB product
#   description with no archive names in it at all, so pointing the parser there
#   yields zero tiles rather than an error.
#
# TAKE /tif/, NOT /xyz/. The same archive name is served under both: /tif/ holds
#   a finished 1000x1000 Float32 GeoTIFF in 1.3 MB, /xyz/ holds 28 MB of ASCII
#   triples needing gdal_translate. Three times the bytes and a conversion pass
#   for the same numbers.
#
# ZIPS ARE STAGED ON NVMe, NOT ON THE DGM DRIVE. 41 GB would be written and
#   then deleted again, and that drive is NTFS over ntfs-3g — every metadata
#   operation is a FUSE round trip. Only the rasters land there.
#
# THE SIDECARS ARE DROPPED. The .tfw repeats an origin and pixel size the
#   GeoTIFF already carries, verified with it removed; the _meta.html is 8 KB of
#   near-identical boilerplate per tile, which would double the file count on
#   the FUSE volume for nothing. Statewide currency is published as two PDFs
#   under /geobasis/information/aktualitaeten/ instead.
#
# PROVENANCE IS MIXED — laser scan in most places, photogrammetric
#   post-processing elsewhere, and the western tile sampled on 2026-09-26 names
#   a 2008 Bildflug. So expect the roughness scan to find smooth, featureless
#   ground that is an artefact of photogrammetry rather than flat terrain, and
#   pick sample sites accordingly.
#
# NODATA IS A DECLARED -9999, uniform over 300 sampled rasters, with no all-zero
#   edge row of the kind Hessen hid in 17 of its own.
#
# 0.00 OCCURS AND IS REAL GROUND — DO NOT MASK IT. Baden-Württemberg writes 0.00
#   for out-of-coverage and needs `-srcnodata 0`; doing that here would punch
#   holes in the Oderbruch, which is genuinely at and below sea level. Measured
#   on the three rasters of 300 that contain a zero: each ranges from about
#   -1.1 m to 6 m, carries 150k-250k pixels within half a metre of zero across
#   99 distinct values, and its zeros are scattered over the whole tile rather
#   than massed in a block or against an edge. Expect a real 0 m contour.
#
# NO CHECKSUMS ARE PUBLISHED, unlike Rheinland-Pfalz's metalink. Integrity rests
#   on the zip CRC, which `unzip -t` checks before extraction, so a truncated
#   archive is dropped and refetched rather than silently yielding a short file.
#
# Resumable: a tile whose raster is already present is not refetched, so a
# re-run costs one directory listing. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-bb.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const INDEX = "https://data.geobasis-bb.de/geobasis/daten/dgm/tif/"
const BASE  = "https://data.geobasis-bb.de/geobasis/daten/dgm/tif"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Brandenburg"
const STAGE = "/mnt/osm/bb-stage"                    # zip buffer + aria2 log, on NVMe
const EPSG  = "EPSG:25833"                           # ETRS89 / UTM zone 33N
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const STAMP = "/home/martin/fm/freemap-outdoor-map/scripts/stamp_crs.py"
const SYS_PYTHON = "/usr/bin/python3"

# Concurrent downloads. 1.3 MB each, so one connection per file and many at
# once; -x4 on a file this size is setup cost for nothing.
const DL_PAR = 24

# Extractors draining the buffer. Extraction is one unzip of one raster, so
# this only has to keep ahead of the link.
const PAR = 6

const EXPECTED = 31291

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-bb.nu"

mkdir $DEST
mkdir $STAGE

# ── Fetch and parse the index ─────────────────────────────────────────────────

let page = $"($STAGE)/index.html"
print "==> fetching the tile index"
(do { ^curl -sS -L --fail --max-time 300 -A $UA -o $page $INDEX } | complete) | ignore
if not ($page | path exists) {
    error make {msg: $"could not fetch ($INDEX)"}
}

let tiles = (
    open --raw $page
      | parse -r 'dgm_33(?<e>\d{3})-(?<n>\d{4})\.zip'
      | uniq
)
print $"==> index lists ($tiles | length) tiles \(expected ($EXPECTED)\)"
if ($tiles | length) < 1000 {
    error make {msg: "index parsed to almost nothing — the listing format has changed"}
}
if ($tiles | length) != $EXPECTED {
    print $"    NOTE: differs from the ($EXPECTED) seen on 2026-09-26 — coverage may have grown"
}

# ── Work out what is missing ──────────────────────────────────────────────────

# PRESENCE, NOT SIZE. Normalising the CRS below rewrites headers in place, so
# sizes stop matching anything the server would report; and one listing of the
# destination is far cheaper than 31291 stats on a FUSE volume.
print "==> working out what is missing"
let present = (
    do { ^find $DEST -maxdepth 1 -name "*.tif" -printf "%f\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|f, acc| $acc | upsert $f true }
)

let pending = ($tiles | where {|t| not ($present | get -o $"dgm_33($t.e)-($t.n).tif" | default false) })
print $"==> ($pending | length) to fetch, ($tiles | length) total"

# ── Download, draining as it goes ─────────────────────────────────────────────

# DOWNLOAD AND EXTRACTION RUN CONCURRENTLY, NOT IN LOCKSTEP — the Baden-
# Württemberg lesson: fetching an archive and then unpacking it in the same
# worker leaves the link idle for as long as the unpacking takes. One aria2c
# fills a buffer and a separate pool drains it.
#
# aria2c marks work in progress with a sibling .aria2 control file and removes
# it on completion, so "a .zip with no .aria2 beside it" is the ready signal.
if ($pending | is-not-empty) {
    let zipdir = $"($STAGE)/zips"
    mkdir $zipdir
    rm -f $"($zipdir)/.dl-done"

    # Archives left by an interrupted run have lost their control file and are
    # truncated, so they can neither be resumed nor trusted. Drop them and they
    # are simply fetched again.
    for z in (glob $"($zipdir)/*.zip") {
        if not ($"($z).aria2" | path exists) {
            if (do { ^unzip -tqq $z } | complete).exit_code != 0 { rm -f $z }
        }
    }

    # A COMPLETE ZIP ALREADY IN THE BUFFER IS NOT REQUESTED AGAIN. `pending` is
    # computed from rasters on disk, so after an interrupted run every buffered
    # archive is still in the queue: the drain loop extracts and deletes it,
    # aria2c then fetches it a second time, and it is extracted again over the
    # raster it just produced. Correct output, wasted bandwidth. The loop globs
    # the directory, so dropping these from the queue still gets them extracted.
    let buffered = (
        glob $"($zipdir)/*.zip"
          | where {|z| not ($"($z).aria2" | path exists) }
          | each {|z| $z | path basename | str replace ".zip" "" }
          | reduce --fold {} {|f, acc| $acc | upsert $f true }
    )

    let queue = ($pending | where {|t| not ($buffered | get -o $"dgm_33($t.e)-($t.n)" | default false) })
    print $"==> ($queue | length) queued for download, ($pending | length) pending"

    ($queue | each {|t| $"($BASE)/dgm_33($t.e)-($t.n).zip" } | str join "\n")
      | save -f $"($zipdir)/urls.txt"

    # THE FETCH GOES IN A SCRIPT FILE SO `&` HAS ONE COMMAND TO BACKGROUND.
    # `bash -c "aria2c ...; touch done &"` backgrounds only the touch and runs
    # aria2c in the foreground, so the drain loop below never starts until the
    # whole download has finished — the zips pile up and the link and the CPU
    # take turns instead of overlapping.
    [
        "#!/bin/sh"
        $"aria2c -i '($zipdir)/urls.txt' -j ($DL_PAR) -x1 -s1 --continue=true --auto-file-renaming=false --user-agent '($UA)' --connect-timeout=20 --timeout=60 --max-tries=5 --retry-wait=5 --console-log-level=error --summary-interval=0 -d '($zipdir)' > '($zipdir)/aria2.log' 2>&1"
        $"touch '($zipdir)/.dl-done'"
    ] | str join "\n" | save -f $"($zipdir)/fetch.sh"

    (do { ^bash -c $"nohup sh '($zipdir)/fetch.sh' > /dev/null 2>&1 &" } | complete) | ignore
    print $"==> downloader started \(($DL_PAR) concurrent\), draining with ($PAR) extractors"

    mut n = 0
    loop {
        let ready = (
            glob $"($zipdir)/*.zip"
              | where {|z| not ($"($z).aria2" | path exists) }
        )

        if ($ready | is-empty) {
            if ($"($zipdir)/.dl-done" | path exists) { break }
            sleep 3sec
            continue
        }

        $ready | par-each -t $PAR {|zip|
            let stem = ($zip | path basename | str replace ".zip" "")

            if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
                print $"  CORRUPT ($stem) — dropping, will refetch"
                rm -f $zip
                return
            }

            # Extract only the raster; -j flattens, -o overwrites a partial.
            let tmp = $"($STAGE)/x_($stem)"
            rm -rf $tmp
            mkdir $tmp
            (do { ^unzip -joqq $zip $"($stem).tif" -d $tmp } | complete) | ignore

            if ($"($tmp)/($stem).tif" | path exists) {
                mv -f $"($tmp)/($stem).tif" $"($DEST)/($stem).tif"
            } else {
                print $"  NO RASTER in ($stem) — leaving the archive for inspection"
            }
            rm -rf $tmp
            rm -f $zip
        } | ignore

        $n += ($ready | length)
        print $"  extracted ($n) / ($pending | length)"
    }
}

# ── Verify, normalise the CRS, build the state VRT ────────────────────────────

let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
let missing = ($tiles | where {|t| not ($"($DEST)/dgm_33($t.e)-($t.n).tif" | path exists) })
print $"==> ($have | length) rasters present; ($missing | length) missing"

if ($missing | is-not-empty) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    first few: ($missing | first 5 | each {|t| $'33($t.e)-($t.n)'} | str join ', ')"
} else {
    # gdalbuildvrt silently drops inputs whose CRS differs from the first, and
    # a COMPOUNDCRS against a plain PROJCRS reads as identical in its error
    # message — 78 of Rheinland-Pfalz's rasters were lost that way.
    print "==> normalising the CRS onto the majority"
    let s = (do { ^$SYS_PYTHON $STAMP $DEST majority 12 } | complete)
    print ($s.stdout | str trim)
    if $s.exit_code != 0 {
        error make {msg: "stamp_crs.py reported failures — fix them before building the VRT"}
    }

    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        gdal build-vrt $have $"($DEST)/all.vrt" --index $"($DEST)/tiles.txt"
    }
    rm -rf $STAGE
    print $"==> Done -> ($DEST)/all.vrt"
}
