#!/usr/bin/env nu

# Fetch the Hamburg DGM1 (1 m digital terrain model) — 880 GeoTIFFs of 1x1 km
# in one 1.4 GB archive.
#
# Source: Landesbetrieb Geoinformation und Vermessung (LGV), via the Hamburg
#   transparency portal.
#   https://daten-hamburg.de/opendata/fernerkundung_hoehenmodelle/dgm/
#   GeoTIFF, Float32, LZW, 1 m, EPSG:25832, survey 2022-03 throughout.
#
# LICENCE dl-de/by-2-0, credit "Freie und Hansestadt Hamburg, Landesbetrieb
#   Geoinformation und Vermessung (LGV)".
#
# THE EASIEST GERMAN SOURCE SO FAR: one archive, already GeoTIFF, one survey
#   date for every tile, one CRS. Earlier vintages back to 2013 are ASCII XYZ;
#   the 2022 release is the first as raster, which is why it is the one used.
#
# THE OLD URLs ARE DEAD. Everything under
#   /geographie_geologie_geobasisdaten/Digitales_Hoehenmodell/DGM1/ now 404s —
#   the files moved to /opendata/fernerkundung_hoehenmodelle/dgm/ and the CKAN
#   record that still lists the old paths is marked for deletion. Follow the
#   "Geländemodelle Hamburg - DGM" record, not "Digitales Höhenmodell Hamburg
#   DGM 1".
#
# THE DECLARED NODATA IS A LIE AND MUST BE OVERRIDDEN. Every tile declares
#   -3.4028235e+38, and not one pixel holds that value; the actual void marker
#   is -9999, which is 21.74% of the sample tile. Trusting the declaration
#   turns a fifth of the city into terrain 9999 m below sea level — the mean of
#   that tile reads -2172 m for exactly this reason. Hence -a_nodata -9999 on
#   the stamp below and -srcnodata on the VRT.
#
# AND THE DECLARED VALUE IS ALSO WRITTEN, IN THREE TILES. Having established
#   that -9999 is the real marker, three of the 880 rasters then use
#   -3.4028235e+38 as well — 972600 pixels in one of them, 97% of the tile.
#   Stamping -9999 as the band's nodata leaves those as valid data at 3.4e38,
#   the shading renders a window of garbage, and the first thing that notices is
#   gdal_contour refusing "too many levels" an hour later. normalise_nodata.py
#   rewrites them to -9999 below, touching only the tiles that carry the value.
#
# 0.00 IS REAL GROUND, as in Schleswig-Holstein and unlike Mecklenburg-
#   Vorpommern: the sample tile carries 506 zero pixels among real values
#   running -1.20 m to 6.97 m, which is Elbe marsh crossing the zero level.
#   Do not mask it.
#
# THE ARCHIVE NESTS TWO DIRECTORIES DEEP and groups tiles by easting:
#   dgm1_hh_2022-04-30/dgm1_hh_2022/S32_466/dgm1_32_466_5973_1_hh_2022.tif
#   so extraction flattens. A CSV alongside carries each tile's survey date,
#   accuracy and CRS; it is kept for reference.
#
# Resumable: a tile already present is not re-extracted, and the archive is
# only refetched if it is missing. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-hh.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const URL   = "https://daten-hamburg.de/opendata/fernerkundung_hoehenmodelle/dgm/dgm1_hh_2022-04-30.zip"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Hamburg"
const STAGE = "/mnt/osm/hh-stage"                    # archive + extraction, on NVMe
const EPSG  = "EPSG:25832"                           # ETRS89 / UTM zone 32N
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const PAR   = 8
const EXPECTED = 880
const NORM = "/home/martin/fm/freemap-outdoor-map/scripts/normalise_nodata.py"
const SYS_PYTHON = "/usr/bin/python3"

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-hh.nu"

mkdir $DEST
mkdir $STAGE

# ── Fetch the archive ─────────────────────────────────────────────────────────

let zip = $"($STAGE)/dgm1_hh_2022.zip"
if not ($zip | path exists) {
    print "==> fetching the archive \(1.4 GB\)"
    (do { ^curl -sS -L --fail --max-time 3600 -A $UA -o $zip $URL } | complete) | ignore
    if not ($zip | path exists) {
        error make {msg: $"could not fetch ($URL)"}
    }
} else {
    print "==> archive already staged"
}

if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
    rm -f $zip
    error make {msg: "archive failed its CRC check and was removed — re-run to refetch"}
}

# ── Extract and stamp ─────────────────────────────────────────────────────────

let work = $"($STAGE)/x"
mkdir $work

let names = (
    do { ^unzip -Z1 $zip } | complete | get stdout | lines
      | where {|n| $n | str ends-with ".tif" }
)
print $"==> ($names | length) rasters in the archive \(expected ($EXPECTED)\)"
if ($names | length) != $EXPECTED {
    print $"    NOTE: differs from the ($EXPECTED) seen on 2026-09-28"
}

let todo = ($names | where {|n| not ($"($DEST)/($n | path basename)" | path exists) })
print $"==> ($todo | length) to extract"

if ($todo | is-not-empty) {
    $todo | par-each -t $PAR {|n|
        let base = ($n | path basename)
        let tmp = $"($work)/($base)"
        (do { ^unzip -joqq $zip $n -d $work } | complete) | ignore
        if not ($tmp | path exists) {
            print $"  MISSING after extract: ($base)"
            return
        }
        # -a_nodata -9999 IS THE WHOLE POINT — see the header. Metadata only,
        # so this rewrites the header and not the pixels.
        let c = (do { ^gdal_edit.py -a_nodata -9999 $tmp } | complete)
        if $c.exit_code != 0 {
            print $"  FAILED stamp ($base)"
            rm -f $tmp
            return
        }
        mv -f $tmp $"($DEST)/($base)"
    } | ignore
}

# ── Normalise the second nodata marker ────────────────────────────────────────

# Three tiles write -3.4028235e+38 where the rest write -9999 — see the header.
# Cheap to re-run: only rasters that actually hold the value are rewritten.
print "==> normalising the second nodata marker"
let nn = (do { ^$SYS_PYTHON $NORM $DEST "below:-1e30" "-9999" 12 } | complete)
print ($nn.stdout | str trim)
if $nn.exit_code != 0 {
    error make {msg: "normalise_nodata.py failed — fix before building the VRT"}
}

# Keep the per-tile metadata table beside the rasters.
let csv = (do { ^unzip -Z1 $zip } | complete | get stdout | lines | where {|n| $n | str ends-with ".csv" })
if ($csv | is-not-empty) {
    (do { ^unzip -joqq $zip ($csv | first) -d $DEST } | complete) | ignore
}

# ── Verify and build the VRT ──────────────────────────────────────────────────

let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
let missing = ($names | where {|n| not ($"($DEST)/($n | path basename)" | path exists) })
print $"==> ($have | length) rasters present; ($missing | length) missing"

if ($missing | is-not-empty) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    first few: ($missing | first 5 | each {|n| $n | path basename } | str join ', ')"
} else {
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # -srcnodata -9999 as well as the stamp: belt and braces, because a
        # tile that somehow escaped the stamp would otherwise contribute
        # terrain at -9999 m and nothing downstream would notice.
        (gdal build-vrt $have $"($DEST)/all.vrt"
           --extra [-srcnodata -9999 -vrtnodata -9999]
           --index $"($DEST)/tiles.txt")
    }
    rm -rf $work
    print $"==> Done -> ($DEST)/all.vrt"
}
