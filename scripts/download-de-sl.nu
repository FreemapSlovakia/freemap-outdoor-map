#!/usr/bin/env nu

# Fetch the Saarland DGM1 (1 m digital terrain model) — 3076 GeoTIFFs of 1x1 km
# in six district archives, 5.1 GB, about 5 GB once recompressed.
#
# Source: Landesamt für Vermessung, Geoinformation und Landentwicklung (LVGL),
#   open data on their Nextcloud.
#   https://www.shop.lvgl.saarland.de/cloud/freiegeobasisdaten?dir=/OD_DGM1_2025_tif_LK
#   GeoTIFF, Float32, uncompressed, 1 m, EPSG:25832, nodata -9999.
#
# LICENCE dl-de/by-2-0, credit "© GeoBasis DE/LVGL-SL".
#
# SURVEYED 2025 — the newest data in the German set, against Rheinland-Pfalz's
#   2022-2024 and Schleswig-Holstein's 2005-2025 mixture.
#
# THE GEOPORTAL WILL TELL YOU THIS DATA CANNOT BE DOWNLOADED. Do not believe
#   it. The metadata for the 2016 edition links to
#   shop.lvgl.saarland.de/cloud/index.php/s/Download_Hinweis/download, which
#   returns a plain-text note saying the record is too large to deliver and to
#   contact their sales department for it on disk. That note is attached to the
#   2016 product; the 2025 edition is published as open data on the same
#   Nextcloud and is catalogued in GovData, not in the state geoportal. The
#   INSPIRE services are no help either: the WCS serves the whole state as one
#   4096x4096 grid, about 18 m a pixel, and reports its unit as W.m-2.Sr-1,
#   while the predefined-dataset ATOM feed answers "Title of dataset cannot be
#   found!" for every DGM1 record id.
#
# THE SHARE TOKEN IS BASE64 IN THE PAGE. The Nextcloud share is public but
#   WebDAV wants the token as the username; the share page carries it in a
#   `sharingToken` input, base64-encoded and quoted. It is read out below
#   rather than pinned, so a reissued share does not silently 404.
#
# SIX ARCHIVES, ONE PER DISTRICT: Merzig-Wadern, Neunkirchen, Stadtverband
#   Saarbrücken, Saarlouis, Saarpfalz-Kreis and St. Wendel. THEY OVERLAP along
#   their shared boundaries: the archives hold 3076 entries between them but
#   only 2775 distinct tiles, the other 301 appearing in two districts each. So
#   the completeness test counts distinct names, not entries — summing the
#   archive listings leaves the run permanently 301 short and the VRT never
#   gets built.
#
# THE DELIVERY IS ALMOST CLEAN: plain PROJCRS EPSG:25832 with no compound
#   variant, nodata honestly declared as -9999, origins exactly on the
#   kilometre grid, no sentinel and no second void marker.
#
# TWO TILES CARRY A SPIKE TO ZERO, AND THEY ARE THE TWO ROUGHEST IN THE STATE.
#   dgm1_32_343_5457 holds 195 pixels running down to -0.26 m and
#   dgm1_32_327_5485 another 121, each a patch about 20x12 px inside ground at
#   roughly 200 m. They are not terrain: Saarland's lowest point is 138 m at
#   Perl on the Moselle, and both tiles bottom out legitimately around 184 m
#   (p0.05). Left alone they produce twenty rings of nonsense contours from 0 m
#   upward and put the two tiles at the top of the roughness ranking, where
#   202.6 m of relief at a 1.8° median slope reads like a spoil heap rather
#   than the artifact it is.
#
#   THE DEFECT SITS BETWEEN THE USUAL TESTS. A scan for exact 0.00 finds 105 of
#   the pixels and misses the rest; a scan for ground below -2 m finds none of
#   them. What catches it is comparing against the state's known floor: in a
#   state whose lowest ground is 138 m, anything under about 130 m is wrong.
#
#   The repair is applied below, per tile, against each tile's own p0.05 rather
#   than a state-wide threshold — a blanket cut would delete real ground at
#   Perl, which is genuinely 137-142 m.
#
# THE TILES ARRIVE UNCOMPRESSED at 4 MB each; DEFLATE with PREDICTOR=3 takes
#   them to 42%, so they are recompressed on the way in. PREDICTOR=3 is safe
#   because this raster is read by GDAL only — shading-de-sl.nu rewrites its
#   window DEM with PREDICTOR=1 for feature-preserving smoothing, which ignores
#   the tag.
#
# Resumable: a tile already converted is not redone, and an archive already
# staged is not refetched. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-sl.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SHARE = "https://www.shop.lvgl.saarland.de/cloud/freiegeobasisdaten?dir=/OD_DGM1_2025_tif_LK"
const DAV   = "https://www.shop.lvgl.saarland.de/cloud/public.php/dav/files"
const DIR   = "OD_DGM1_2025_tif_LK"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Saarland"
const STAGE = "/mnt/osm/sl-stage"                    # archives + extraction, on NVMe
const EPSG  = "EPSG:25832"                           # ETRS89 / UTM zone 32N
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const PAR   = 8
const SYS_PYTHON = "/usr/bin/python3"
const MASK = "/home/martin/fm/freemap-outdoor-map/scripts/mask_below.py"
const EXPECTED = 2775          # DISTINCT tiles; the archives list 3076 with overlap

const DISTRICTS = ["MZG" "NK" "SB" "SLS" "SPK" "WND"]

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-sl.nu"

mkdir $DEST
mkdir $STAGE

let work = $"($STAGE)/x"
mkdir $work

# ── Resolve the share token ───────────────────────────────────────────────────

print "==> reading the share token"
let page = (do { ^curl -sS -L --fail --max-time 120 -A $UA $SHARE } | complete | get stdout)
let enc = ($page | parse -r 'sharingToken"\s+value="([^"]+)"' | get capture0.0?)
if ($enc | is-empty) {
    error make {msg: "could not find sharingToken on the share page — the share may have been reissued"}
}
let token = (
    $enc | decode base64 | decode utf-8 | str trim --char '"'
)
print $"==> token ($token)"

# ── Fetch, extract, recompress ────────────────────────────────────────────────

mut seen = []

for d in $DISTRICTS {
    let zip = $"($STAGE)/($d).zip"
    let url = $"($DAV)/($token)/($DIR)/DGM1_tif_($d)_EPSG-25832_Entstehung-2025.zip"

    if not ($zip | path exists) {
        print $"==> fetching ($d)"
        (do { ^curl -sS -L --fail --max-time 3600 -u $"($token):" -A $UA -o $zip $url } | complete) | ignore
    }
    if not ($zip | path exists) {
        error make {msg: $"could not fetch ($url)"}
    }
    if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
        rm -f $zip
        error make {msg: $"($d).zip failed its CRC check and was removed — re-run to refetch"}
    }

    let names = (
        do { ^unzip -Z1 $zip } | complete | get stdout | lines
          | where {|n| $n | str ends-with ".tif" }
    )
    $seen = ($seen | append ($names | each {|n| $n | path basename }))

    let todo = ($names | where {|n| not ($"($DEST)/($n | path basename)" | path exists) })
    print $"==> ($d): ($names | length) tiles, ($todo | length) to convert"

    if ($todo | is-not-empty) {
        $todo | par-each -t $PAR {|n|
            let base = ($n | path basename)
            let raw = $"($work)/($base)"
            let out = $"($DEST)/($base)"

            (do { ^unzip -joqq $zip $n -d $work } | complete) | ignore
            if not ($raw | path exists) {
                print $"  MISSING after extract: ($base)"
                return
            }
            let c = (do {
                ^gdal_translate -q -of GTiff -ot Float32 -a_srs $EPSG -co COMPRESS=DEFLATE -co PREDICTOR=3 -co ZLEVEL=6 -co TILED=YES $raw $"($out).tmp"
            } | complete)
            rm -f $raw
            if $c.exit_code == 0 and ($"($out).tmp" | path exists) {
                mv -f $"($out).tmp" $out
            } else {
                rm -f $"($out).tmp"
                print $"  FAILED convert ($base)"
            }
        } | ignore
    }
}

# ── Repair the two spike tiles ────────────────────────────────────────────────

# See the header. Each is masked against its own floor, not a state-wide one.
print "==> repairing the two known spike tiles"
for r in [{tile: "dgm1_32_343_5457_1_SL_2025.tif", below: 170}
          {tile: "dgm1_32_327_5485_1_SL_2025.tif", below: 160}] {
    let f = $"($DEST)/($r.tile)"
    if ($f | path exists) {
        let out = (do { ^$SYS_PYTHON $MASK $f $r.below } | complete)
        print $"   ($r.tile): masked ($out.stdout | str trim) px below ($r.below) m"
    }
}

# GDAL caches band statistics in a .aux.xml beside each raster and keeps
# serving them after the pixels change, so anything reading statistics — a
# later gdalinfo, or gdal_contour deciding how many levels to cut — would act
# on the values from before the repair.
(do { ^bash -c $"find '($DEST)' -name '*.aux.xml' -delete" } | complete) | ignore

# ── Verify and build the state VRT ────────────────────────────────────────────

let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
let distinct = ($seen | uniq)
print $"==> ($have | length) rasters present; ($distinct | length) distinct tiles across the six archives \(expected ($EXPECTED)\)"

if ($have | length) < ($distinct | length) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
} else {
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # nodata is declared honestly in the delivery, so nothing to override.
        gdal build-vrt $have $"($DEST)/all.vrt" --index $"($DEST)/tiles.txt"
    }
    rm -rf $work
    print $"==> Done -> ($DEST)/all.vrt"
}
