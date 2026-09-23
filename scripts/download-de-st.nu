#!/usr/bin/env nu

# Fetch the Saxony-Anhalt DGM1 (1 m digital terrain model) — 5465 tiles of 2x2 km.
#
# Source: LVermGeo Sachsen-Anhalt, "Kostenfreies Digitales Geländemodell mit
#   einer Rasterweite von 1 m (DGM1) — landesweit". GeoTIFF, EPSG:25832 (ETRS89 /
#   UTM 32N), heights DE_DHHN2016_NH, nodata -9999, LZW, accuracy 0.15 m.
#   https://www.lvermgeo.sachsen-anhalt.de/de/gdp-dgm1-landesweit.html
#
# LICENCE IS dl-de/by-2-0, NOT CC BY 4.0. Every other German state here is
#   CC BY; this one is Datenlizenz Deutschland – Namensnennung – Version 2.0.
#   The attribution that must reach source.json is the per-tile copyright note
#   from the .meta sidecars: "©LVermGeo Sachsen-Anhalt".
#
# FOUR STATEWIDE ZIPS, NOT A TILE INDEX. Unlike Saxony there is no per-tile
#   share and no index to scrape: the whole state ships as DGM1_1..4.zip, ~42 GB
#   together. They are a SPATIAL PARTITION — 5465 distinct tile names with no
#   key appearing twice — so there is no newest-per-tile selection to make here,
#   unlike Niedersachsen where 41% of keys had several vintages.
#
# aria2c, NOT curl. The server throttles PER CONNECTION, not per client: a
#   single curl sat at ~4 MB/s while aria2c with 8 connections per file reached
#   141 MB/s. That is the difference between 20 minutes and three hours.
#
# VALIDATE WITH unzip -t AND Content-Length, NOT SIZE ALONE. Sizes observed
#   2026-09-22: 11538895973 / 11596939008 / 10285778142 / 8629212980.
#
# 40 TILES SHIP WITHOUT A CRS TAG, AND THEY ARE IN THE HARZ. gdalbuildvrt does
#   not fail on these — it prints "heterogeneous projection ... Skipping" into
#   its progress bar and exits 0, so a VRT built naively is quietly short of the
#   state's only mountains. They are georeferenced correctly and only lack the
#   projection, so stamping EPSG:25832 on them is safe and loses nothing. The
#   build below goes through `gdal build-vrt`, which checks every offered input
#   back out of the VRT and errors rather than returning a short one.
#
# Resumable: a zip that already validates is not re-fetched, extraction is
# skipped once the tile count matches, and the VRT is rebuilt only if absent.
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-st.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const BASE = "https://www.geodatenportal.sachsen-anhalt.de/gfds_webshare/download/LVermGeo/Geodatenportal/Online-Bereitstellung-LVermGeo/DGM"
const DEST = "/run/media/martin/2190983A5767510F/DGM1/Sachsen-Anhalt"
const EXPECTED = 5465
const EPSG = "EPSG:25832"

const PARTS = [
    {name: "DGM1_1.zip", size: 11538895973}
    {name: "DGM1_2.zip", size: 11596939008}
    {name: "DGM1_3.zip", size: 10285778142}
    {name: "DGM1_4.zip", size: 8629212980}
]

gdal assert-mounted $DEST

# ── Fetch ─────────────────────────────────────────────────────────────────────

for part in $PARTS {
    let zip = $"($DEST)/($part.name)"

    if ($zip | path exists) and (ls $zip | get 0.size | into int) == $part.size {
        print $"==> ($part.name) already complete"
        continue
    }

    print $"==> fetching ($part.name)"
    let dl = (do {
        ^aria2c -x8 -j4 -s8 --continue=true --auto-file-renaming=false --summary-interval=30 -d $DEST -o $part.name $"($BASE)/($part.name)"
    } | complete)

    if $dl.exit_code != 0 {
        error make {msg: $"aria2c failed on ($part.name) — re-run to resume"}
    }

    let got = (ls $zip | get 0.size | into int)
    if $got != $part.size {
        error make {msg: $"($part.name) is ($got) bytes, expected ($part.size)"}
    }
    if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
        error make {msg: $"($part.name) failed unzip -t despite the right size"}
    }
    print $"  ok — ($got) bytes, archive intact"
}

# ── Extract ───────────────────────────────────────────────────────────────────

# The zips carry a DGM1_<n>/ prefix; flatten so every state looks the same on
# disk and the shading/sampler scripts can glob one directory.
let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines | length)

if $have < $EXPECTED {
    for part in $PARTS {
        print $"==> extracting ($part.name)"
        (do { ^unzip -oqq $"($DEST)/($part.name)" -d $DEST } | complete) | ignore
    }
    for d in (glob $"($DEST)/DGM1_*" | where {|p| ($p | path type) == "dir" }) {
        (do { ^bash -c $"mv '($d)'/* '($DEST)'/ 2>/dev/null; rmdir '($d)'" } | complete) | ignore
    }
}

let tifs = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
print $"==> ($tifs | length) / ($EXPECTED) tifs present"

if ($tifs | length) != $EXPECTED {
    error make {msg: $"expected ($EXPECTED) tiles, found ($tifs | length)"}
}

# ── Stamp the missing CRS ─────────────────────────────────────────────────────

let nocrs = ($tifs | par-each {|f|
    let srs = (do { ^gdalsrsinfo -o epsg $f } | complete | get stdout)
    if ($srs | str contains "25832") { null } else { $f }
} | compact)

if ($nocrs | is-not-empty) {
    print $"==> stamping ($EPSG) on ($nocrs | length) tiles that ship without one"
    $nocrs | par-each {|f| (do { ^gdal_edit.py -a_srs $EPSG $f } | complete) | ignore }
}

# ── Build the state VRT ───────────────────────────────────────────────────────

if not ($"($DEST)/all.vrt" | path exists) {
    print "==> building all.vrt"
    gdal build-vrt $tifs $"($DEST)/all.vrt" --index $"($DEST)/tiles.txt"
}

print $"==> Done -> ($DEST)/all.vrt"
