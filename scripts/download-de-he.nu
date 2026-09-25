#!/usr/bin/env nu

# Fetch the Hessen DGM1 (1 m digital terrain model) — 27 district packages,
# 66.4 GB, holding about 21000 rasters of 1x1 km.
#
# Source: HVBG (Hessische Verwaltung für Bodenmanagement und Geoinformation),
#   Geodaten online Downloadcenter. GeoTIFF, Float32, LZW, nodata -9999,
#   ETRS89 / UTM 32N, heights DHHN2016.
#   https://gds.hessen.de -> Downloadcenter -> 3D-Daten -> DGM1
#
# LICENCE dl-de/zero-2-0 — Datenlizenz Deutschland Zero, which asks for no
#   attribution at all. We credit anyway, with the customary source note:
#   "Geobasisdaten © Hessische Verwaltung für Bodenmanagement und
#   Geoinformation". Open since 2022 under Hessen's own open-geodata act
#   rather than a per-product licence.
#
# THE PORTAL HAS A CLEAN JSON API, found by watching what its own page calls:
#
#     /INTERSHOP/rest/WFS/HLBG-Geodaten-Site/-/downloadcenter?path=<p>&navigation=all
#
#   It lists the districts under DGM1 and, per district, one `packages` entry
#   holding the whole Landkreis. No scraping, no per-tile requests, no UI cap:
#   27 downloads cover the state.
#
# THE PACKAGE URL IS DATE-STAMPED and must be read from the API each run —
#   /downloadcenter/20260924/3D-Daten/... — so it cannot be cached in this
#   script the way Saxony's tile list is.
#
# ARCHIVES NEST TWICE: the district package holds one zip per municipality,
#   and those hold the rasters. Extraction therefore goes two levels deep.
#
# A THIRD OF THE RASTERS SHIP WITHOUT A CRS — 7389 of 22776, measured. The rest
#   carry EPSG:25832 properly, and one municipality can be entirely one or
#   entirely the other, so a sample of a few tiles will mislead either way.
#   The georeference itself is always present and correct (origin on integer km,
#   1 m pixels); only the projection is missing.
#
#   MIXED IS THE DANGEROUS CASE. Had they all been blank, gdalbuildvrt would
#   yield a VRT with no CRS and the warp would fail loudly. Mixed, it keeps the
#   majority and SKIPS the rest with a warning swallowed by its progress bar —
#   a third of Hessen quietly absent from the mosaic, which is how
#   Baden-Württemberg lost its 40 Harz tiles. stamp_crs.py writes EPSG:25832
#   into the files that lack it, before the VRT is built; it is metadata only
#   and reads no pixels. `gdal build-vrt` then checks every input back out.
#
# THE .tfw AND .aux.xml SIDECARS ARE REDUNDANT and are not kept: a raster reads
#   with the right origin on its own, verified with the sidecars removed.
#
# NODATA IS -9999 AND DECLARED, so unlike Baden-Württemberg there is no
#   -srcnodata on the VRT and no fringe prefilter.
#
#   BUT 17 RASTERS CARRY A ZERO TOP ROW, repaired in place on 2026-09-25. Each
#   held 1000 pixels of 0.00 across row 0 with real ground in row 1 — a 386 m
#   cliff in one pixel — all of them on the state's northern edge (northings
#   5693 to 5721). 0.000075% of the state, and the only symptom downstream was
#   a 60 m contour in a state whose floor is about 81 m. They are now -9999.
#   If this dataset is ever re-fetched, re-check: remove the .done markers for
#   those districts and the download will pull the originals back.
#
# ARCHIVES ARE STAGED ON NVMe, NOT ON THE DGM DRIVE. They are transit: writing
#   66 GB of zip to the DGM drive and reading it straight back saturates
#   it and starves the download, measured on Baden-Württemberg at 6 MB/s
#   against 43 MB/s once the buffer moved. Only the finished rasters land on
#   the slow disk.
#
# aria2c NEEDS A BROWSER USER-AGENT; with its default one every request fails.
#
# Resumable: one marker per district under .done, so a re-run costs nothing.
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-he.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const HOST  = "https://gds.hessen.de"
const REST  = "https://gds.hessen.de/INTERSHOP/rest/WFS/HLBG-Geodaten-Site/-/downloadcenter"
const ROOT  = "3D-Daten/Digitales Geländemodell (DGM1)"
const MOUNT = "/run/media/martin/2190983A5767510F"
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Hessen"
const STAGE = "/mnt/osm/he-stage"                  # archives, on NVMe
const EPSG  = "EPSG:25832"
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const STAMP = "/home/martin/fm/freemap-outdoor-map/scripts/stamp_crs.py"
const SYS_PYTHON = "/usr/bin/python3"
const EXPECTED_DISTRICTS = 27

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-he.nu"

if not ($STAMP | path exists) {
    error make {msg: $"($STAMP) not found — every Hessen raster ships without a CRS and must be stamped; see the header"}
}

mkdir $DEST
mkdir $"($DEST)/.done"
mkdir $STAGE

# ── Enumerate the districts ───────────────────────────────────────────────────

def api [path: string]: nothing -> record {
    let url = $"($REST)?path=($path | url encode)&navigation=all"
    http get --headers [User-Agent $UA Accept "application/json"] $url
}

# `navigation` is a FLAT list of {name, uri, id, level, parentId}, not a tree,
# so the districts are the nodes whose parentId is the DGM1 node's id. Reading
# them by position in the list instead would silently pick up the neighbouring
# DOM1 and document categories whenever the portal reorders itself.
print "==> listing districts"
let nav = (api $ROOT | get navigation)
let dgm = ($nav | where name == "Digitales Geländemodell (DGM1)" | get -o 0)
if $dgm == null {
    error make {msg: "DGM1 node not found in the download-centre navigation — the portal layout changed"}
}
let districts = ($nav | where {|n| ($n | get -o parentId) == $dgm.id } | get name)

print $"==> ($districts | length) districts \(expected ($EXPECTED_DISTRICTS)\)"
if ($districts | length) != $EXPECTED_DISTRICTS {
    error make {msg: $"found ($districts | length) districts, expected ($EXPECTED_DISTRICTS) — check the portal before trusting this list: ($districts | str join ', ')"}
}

# ── Fetch, unpack, stamp ──────────────────────────────────────────────────────

let pending = ($districts | where {|n| not ($"($DEST)/.done/($n)" | path exists) })
print $"==> ($pending | length) pending, ($districts | length) total"

for n in $pending {
    print $"==> ($n)"
    let r = (api $"($ROOT)/($n)")
    let pkgs = ($r.searchresult | get -o packages | default [])
    if ($pkgs | is-empty) {
        print $"  NO PACKAGE for ($n) — skipping, re-run once the portal has one"
        continue
    }
    let pkg = ($pkgs | first)
    let zip = $"($STAGE)/($n).zip"
    print $"  ($pkg.fileSize), built ($pkg.creationDate)"

    if ($zip | path exists) and (do { ^unzip -tqq $zip } | complete).exit_code != 0 { rm -f $zip }

    if not ($zip | path exists) {
        # The uri holds spaces and umlauts; encode the path, keep the slashes.
        let enc = ($pkg.downloadLink.uri | split row "/" | each {|s| $s | url encode } | str join "/")
        let dl = (do { ^aria2c -x4 -s4 --continue=true --auto-file-renaming=false --user-agent $UA --connect-timeout=20 --timeout=60 --max-tries=5 --retry-wait=5 --console-log-level=error --summary-interval=0 -d $STAGE -o $"($n).zip" $"($HOST)($enc)" } | complete)
        if $dl.exit_code != 0 {
            print $"  FAILED download ($n)"
            continue
        }
    }

    if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
        print $"  CORRUPT ($n) after fetch — skipping"
        rm -f $zip
        continue
    }

    let work = $"($STAGE)/work_($n)"
    rm -rf $work
    mkdir $work
    (do { ^unzip -oqq $zip -d $work } | complete) | ignore

    # Second level: one archive per municipality, rasters inside. Only the
    # .tif is taken — the .tfw and .aux.xml add nothing.
    for inner in (glob $"($work)/**/*.zip") {
        (do { ^unzip -oqq -j $inner "*.tif" -d $DEST } | complete) | ignore
    }

    rm -rf $work
    rm -f $zip
    touch $"($DEST)/.done/($n)"
}

# ── Stamp the CRS, verify, build the state VRT ────────────────────────────────

let missing = ($districts | where {|n| not ($"($DEST)/.done/($n)" | path exists) })
let tifs = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
print $"==> ($tifs | length) rasters from ($districts | length) districts; ($missing | length) districts unprocessed"

if ($missing | is-not-empty) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    ($missing | str join ', ')"
} else {
    print $"==> stamping ($EPSG) — every Hessen raster ships without one"
    let s = (do { ^$SYS_PYTHON $STAMP $DEST ($EPSG | str replace "EPSG:" "") 12 } | complete)
    print ($s.stdout | str trim)
    if $s.exit_code != 0 {
        error make {msg: "stamp_crs.py reported failures — fix them before building the VRT"}
    }

    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        gdal build-vrt $tifs $"($DEST)/all.vrt" --index $"($DEST)/tiles.txt"
    }
    rm -rf $STAGE
    print $"==> Done -> ($DEST)/all.vrt"
}
