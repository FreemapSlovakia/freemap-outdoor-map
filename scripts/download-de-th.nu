#!/usr/bin/env nu

# Fetch the Thuringian DGM1 (1 m digital terrain model), 1x1 km tiles.
#
# Source: TLBG (Thüringer Landesamt für Bodenmanagement und Geoinformation) via
#   the GDI-Th INSPIRE Atom download service. GeoTIFF/XYZ, EPSG:25832 (ETRS89 /
#   UTM 32N), heights DHHN2016 (EPSG:7837) on quasigeoid GCG2016, stated
#   accuracy 0.15-0.30 m. Licence: Datenlizenz Deutschland - Namensnennung -
#   Version 2.0 (dl-de/by-2-0), attribution "GDI-Th, Freistaat Thüringen".
#
# THE FEED IS READ AT RUNTIME, unlike Saxony's hardcoded tile table. Thuringia
#   publishes a proper INSPIRE Atom service, so there is nothing to scrape and
#   nothing to go stale: the dataset feed lists every tile as a direct zip URL.
#   No captcha, no token, no form. (The HTML download page does sit behind a
#   Link11 captcha — ignore it and use the feed.)
#
# TWO VINTAGES ARE COMBINED, AND THIS IS THE POINT OF THE SCRIPT.
#   The feed offers three datasets:
#     2010-2013  DGM2, 2 m grid,  17127 tiles  — ignored, too coarse
#     2014-2019  DGM1, 1 m grid,  17127 tiles  — "vollständig vorhanden"
#     2020-2025  DGM1, 1 m grid,  16945 tiles  — "im Aufbau", newest
#   The 2020-2025 set is a STRICT SUBSET of 2014-2019: 182 tiles short, none
#   exclusive to it. So the newest survey is taken wherever it exists and the
#   remaining 182 come from 2014-2019, giving complete 1 m coverage at the best
#   available vintage. The union is exactly 17127 tiles.
#
# THE TWO VINTAGES USE DIFFERENT COORDINATE CONVENTIONS — HALF A PIXEL APART.
#   MEASURED 2026-09-18 on tile 566_5611, which exists in both:
#
#     2020-2025 xyz:  566000.50 5611999.50 407.46   <- pixel CENTRES
#     2014-2019 xyz:  566000.00 5611999.00 407.46   <- cell CORNERS
#
#   Same cell, same height, labelled differently. Shifting the old grid by
#   +0.5/+0.5 matches the new grid on all 200000 sampled cells (median |diff|
#   0.050 m, i.e. genuine survey-to-survey noise); unshifted, ZERO cells
#   coincide. GDAL's XYZ driver assumes centres, so converting the old tiles
#   naively places them half a metre off every neighbour — a seam that would
#   survive into the hillshade and the contours.
#
#   Hence the old tiles are georeferenced EXPLICITLY from the tile key with
#   -a_ullr rather than from the xyz coordinates. Do not "simplify" that away.
#
# ONLY THE NEW VINTAGE SHIPS A GeoTIFF. Its zip holds .tif + .xyz + .meta and
#   the .tif is taken directly (LZW, Float32, nodata -9999, 1000x1000 @ 1 m).
#   The 2014-2019 zips hold .xyz + .meta only, so those 182 are converted here.
#   Each zip carries a 29 MB xyz we do not need but cannot avoid downloading:
#   about 146 GB of traffic for roughly 56 GB of kept GeoTIFFs.
#
# PARALLELISM: 12 measured safe (24/24 HTTP 200, ~20 MB/s aggregate). Unlike
#   Saxony's host, this one does not throttle, and in particular does not
#   disguise throttling as 404. Raise cautiously and watch for non-200s.
#
# Resumable: a tile whose output .tif already exists is skipped, so re-running
# after an interruption is cheap. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-th.nu

# ONE TILE IS UNCONVERTIBLE AND IS SKIPPED ON PURPOSE. 736_5636 (2014-2019) is
# a ragged border sliver: 2995 points scattered over a 24x316 m bounding box,
# stored column-major, so it is not the complete rectangle GDAL's XYZ driver
# needs — it refuses with "Ungridded dataset ... Y spacing was -222.000000".
# Re-sorting does not help; only hand-gridding would, for 0.003 km2 of data on
# the Saxony border. Listed here so the run can report complete instead of
# failing forever on a tile that will never convert.
const SKIP = ["dgm1_736_5636_1_th_2014-2019"]

const FEED = "https://geoportal.geoportal-th.de/dienste/atom_th_hoehendaten_dgm?type=dataset&id=14418d25-fcd7-4a3f-99a9-e3059a2772af"
const DEST = "/run/media/martin/2190983A5767510F/DGM1/Thueringen"
const PAR  = 12
const TRIES = 4
const EPSG = "EPSG:25832"

def tile-key [name: string]: nothing -> string {
    # dgm1_32_561_5609_1_th_2020-2025  ->  561_5609
    # dgm1_561_5609_1_th_2014-2019     ->  561_5609
    let p = ($name | split row "_")
    let i = if ($p | get 1) == "32" { 2 } else { 1 }
    $"($p | get $i)_($p | get ($i + 1))"
}

print "==> Fetching the Atom dataset feed"
let xml = (http get $FEED)

# Entries in feed order: 2010-2013 (2 m), 2014-2019 (1 m), 2020-2025 (1 m).
# Split on <entry> rather than parsing XML — the feed is 13 MB and flat.
let entries = ($xml | split row "<entry>" | skip 1)

def links [entry: string]: nothing -> list<string> {
    $entry
      | split row 'rel="section" href="'
      | skip 1
      | each {|s| $s | split row '"' | first }
      | where {|u| $u | str ends-with ".zip" }
}

let new_urls = (links ($entries | get 2))
let old_urls = (links ($entries | get 1))
print $"  2020-2025: ($new_urls | length) tiles"
print $"  2014-2019: ($old_urls | length) tiles"

let new_keys = ($new_urls | each {|u| tile-key ($u | path basename | path parse | get stem) })
let gap_urls = (
    $old_urls | where {|u| not (($new_keys) has (tile-key ($u | path basename | path parse | get stem))) }
)
print $"  gaps filled from 2014-2019: ($gap_urls | length)"
print $"  total: (($new_urls | length) + ($gap_urls | length)) tiles"

mkdir $DEST

# One record per tile: url, the output tif, and whether it needs xyz conversion.
let jobs = (
    ($new_urls | each {|u|
        let stem = ($u | path basename | path parse | get stem)
        {url: $u, stem: $stem, tif: $"($DEST)/($stem).tif", convert: false, key: (tile-key $stem)}
    })
    | append ($gap_urls | each {|u|
        let stem = ($u | path basename | path parse | get stem)
        {url: $u, stem: $stem, tif: $"($DEST)/($stem).tif", convert: true, key: (tile-key $stem)}
    })
)

let wanted = ($jobs | where {|j| not ($SKIP has $j.stem) })
if ($wanted | length) < ($jobs | length) {
    print $"  skipping (($jobs | length) - ($wanted | length)) known-unconvertible tile\(s\) — see SKIP"
}

let todo = ($wanted | where {|j| not ($j.tif | path exists) })
print $"==> ($todo | length) pending of ($wanted | length)"

$todo | par-each -t $PAR {|j|
    let zip = $"($DEST)/($j.stem).zip"
    mut ok = false
    for attempt in 1..$TRIES {
        let r = (do { ^curl -sfL --max-time 600 -o $zip $j.url } | complete)
        # Validate the ARCHIVE, never the size. Border tiles are legitimately
        # tiny — 576_5699 is 8 KB holding 1936 points, a 44x44 m sliver of its
        # 1x1 km square — and a size threshold rejects them as failures.
        let intact = if ($zip | path exists) {
            (do { ^unzip -tqq $zip } | complete).exit_code == 0
        } else {
            false
        }
        if $r.exit_code == 0 and $intact {
            $ok = true
            break
        }
        sleep (($attempt * 3) | into duration --unit sec)
    }
    if not $ok {
        print $"  !! ($j.stem): download failed after ($TRIES) tries"
        rm -f $zip
        return
    }

    if $j.convert {
        # 2014-2019: xyz only, and its coordinates are CELL CORNERS. Georeference
        # from the tile key so the grid matches the 2020-2025 tiles exactly.
        (do { ^unzip -oqq $zip -d $DEST } | complete) | ignore
        let xyz = $"($DEST)/($j.stem).xyz"
        if not ($xyz | path exists) {
            print $"  !! ($j.stem): no .xyz in archive"
            rm -f $zip
            return
        }
        let raw = $"($j.tif).raw.tif"
        let tmp = $"($j.tif).tmp"
        # One line: nu mis-parses a multi-line external call inside `do {}`.
        (do { ^gdal_translate -q -of GTiff -a_srs $EPSG $xyz $raw } | complete) | ignore
        if ($raw | path exists) {
            # GDAL infers the grid from the xyz and reads its coordinates as pixel
            # centres, so the result sits half a pixel off the 2020-2025 tiles.
            # Shift the georeference rather than forcing the tile's nominal extent:
            # border tiles are SPARSE (one is 188x160 m of a 1x1 km tile), and
            # -a_ullr at the nominal corners would stretch those to 5 m pixels.
            let info = (gdalinfo -json $raw | from json)
            let gt = $info.geoTransform
            let ulx = (($gt | get 0) + 0.5)
            let uly = (($gt | get 3) + 0.5)
            let lrx = ($ulx + ($info.size.0 * ($gt | get 1)))
            let lry = ($uly + ($info.size.1 * ($gt | get 5)))
            let ullr = [$ulx, $uly, $lrx, $lry]
            (do { ^gdal_edit.py -a_ullr ...$ullr $raw } | complete) | ignore

            # THE XYZ DRIVER FILLS UNLISTED CELLS WITH 0, NOT NODATA, and these
            # tiles are overwhelmingly ragged border slivers — measured 60.23%
            # zero across all 181. Thuringia's lowest ground is 113.55 m, so a 0
            # is always fill; left alone it becomes a sea-level cliff in the
            # hillshade and a false contour. This is Poland's zero-nodata trap
            # arriving by a different route.
            # --format is REQUIRED: gdal_calc.py guesses the driver from the
            # output extension and dies with "Cannot guess driver" on the .tmp
            # name used to keep partial writes out of the resume scan.
            (do { ^gdal_calc.py -A $raw --outfile $tmp --format GTiff --calc "where(A==0,-9999,A)" --NoDataValue=-9999 --type=Float32 --co COMPRESS=LZW --co TILED=YES --quiet } | complete) | ignore
            if ($tmp | path exists) { mv $tmp $j.tif }
        }
        rm -f $raw
        rm -f $xyz
        rm -f $"($DEST)/($j.stem).meta"
    } else {
        # 2020-2025: take the provider's own GeoTIFF, discard the 29 MB xyz.
        (do { ^unzip -oqq -j $zip $"($j.stem).tif" -d $DEST } | complete) | ignore
    }
    if not ($j.tif | path exists) {
        print $"  !! ($j.stem): downloaded but produced no tif"
    }
    rm -f $zip
}

let have = (glob $"($DEST)/*.tif" | length)
print $"==> ($have) of ($wanted | length) tiles present"

if $have < ($wanted | length) {
    print "    Re-run to retry the missing ones; finished tiles are skipped."
} else {
    print "==> Building all.vrt"
    let idx = $"($DEST)/tiles.txt"
    glob $"($DEST)/*.tif" | str join "\n" | save -f $idx
    gdalbuildvrt -input_file_list $idx $"($DEST)/all.vrt" o> /dev/null
    rm -f $idx
    print $"==> Done -> ($DEST)/all.vrt"
}
