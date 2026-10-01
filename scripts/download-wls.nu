#!/usr/bin/env nu

# Fetch the Welsh Government LiDAR DTM — one national Cloud-Optimised GeoTIFF of
# 191007x233000 px at 1 m, 45.3 GB on the wire and the same on disk.
#
# Source: Welsh Government / Natural Resources Wales, via DataMapWales.
#   https://datamap.gov.wales/maps/lidar-data-download/
#   COG, Float32, DEFLATE, 256x256 blocks, 1 m, EPSG:27700, nodata -9999.
#
# LICENCE Open Government Licence v3.0, credit "Welsh Government".
#
# ONE FILE, NOT A TILE INDEX, which is unique in this repository. The portal
#   offers per-OS-tile downloads too, but the pre-built national mosaic is a
#   single COG with six overview levels, so there is no index to scrape, no
#   completeness arithmetic, and nothing to discover missing at the end. The
#   32-bit product is the one to take; the 16-bit one is centimetre-quantised.
#
# FLOWN 2020-2022 ACROSS THE WHOLE COUNTRY, which is why this is worth doing at
#   all: unlike Scotland, whose national programme runs to July 2027 and was 15%
#   complete in January 2026, Wales is finished and uniform.
#
# 0.00 IS REAL GROUND AND MUST NOT BE MASKED. This is the Schleswig-Holstein
#   case, not Denmark's. Measured on the coarsest overview before downloading
#   anything: only 147 pixels are exactly 0.00, 0.003% of the data, and the
#   distribution is smooth across the datum — 16002 samples in [-1,0) against
#   17805 in [0,1). Nodata is declared honestly and covers 51.3% of the
#   rectangle, which is the sea and the land outside Wales. There is no sentinel
#   to find and nothing to normalise.
#
# THE DATUM IS THE ONE REAL HAZARD, AND IT IS SHARED WITH ENGLAND. EPSG:27700
#   sits on OSGB36; reaching WGS84 needs OSTN15, OS's NTv2 grid. PROJ does not
#   error when that grid is absent — it silently falls back to a 7-parameter
#   Helmert, which in Wales lands 0.61-1.97 m away (Snowdon 1.86 m, Anglesey
#   1.97 m, Cardiff 0.61 m).
#
#   OSTN15 is installed box-wide since 2026-08-17, so anything built now is
#   correct. England's shading and contours were built before that and carry the
#   Helmert error, about 1.9 m. SO WALES AND ENGLAND WILL NOT AGREE ALONG THEIR
#   BORDER until England is rebuilt: expect roughly 1.3 px of disagreement at
#   z16 over the 250 km boundary, with contour lines not quite joining. That is
#   Wales being right and England being wrong, not the reverse. See the datum
#   section of contours-en.nu for the rebuild, which needs no setup.
#
# RESUMABLE BY BYTE RANGE. The blob answers HTTP 206, so aria2c resumes a part
#   file rather than restarting 45 GB. There is no per-tile granularity to fall
#   back on, which is why the size is checked against the server's own
#   Content-Length before the file is trusted.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-wls.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const URL   = "https://dmwproductionblob.blob.core.windows.net/cogs/lidar/wales_dtm_32bit_cog.tif"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Wales"
const TIF   = "wales_dtm_1m.tif"
const EPSG  = "EPSG:27700"                           # OSGB36 / British National Grid
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const CONN  = 8

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-wls.nu"

mkdir $DEST

let out = $"($DEST)/($TIF)"

# ── Ask the server how big it is ──────────────────────────────────────────────

print "==> asking the server for the size"
let head = (do { ^curl -sSI -A $UA --max-time 120 $URL } | complete | get stdout)
let want = (
    $head | lines | where {|l| ($l | str lowercase) starts-with "content-length:" }
      | first | split row ":" | last | str trim | into int
)
if $want < 1000000000 {
    error make {msg: $"Content-Length came back as ($want) — the URL or the blob has changed"}
}
print $"==> ($want) bytes \(($want / 1073741824 | math round --precision 1) GB\)"

# ── Fetch ─────────────────────────────────────────────────────────────────────

let have = (if ($out | path exists) { ls $out | get size.0 | into int } else { 0 })

if $have == $want {
    print "==> already complete"
} else {
    if $have > 0 {
        print $"==> resuming at ($have / 1073741824 | math round --precision 1) GB"
    }
    # -c resumes; the blob answers 206 so the ranges are real, not a re-fetch.
    # One line: an external command cannot span newlines in nushell.
    (do { ^aria2c -x $CONN -s $CONN -c --file-allocation=none --console-log-level=warn --summary-interval=30 -U $UA -d $DEST -o $TIF $URL } | complete) | ignore
}

let got = (if ($out | path exists) { ls $out | get size.0 | into int } else { 0 })
if $got != $want {
    error make {msg: $"($out) is ($got) bytes, expected ($want) — re-run to resume"}
}
print "==> size matches the server"

# ── Verify before trusting it ─────────────────────────────────────────────────

# A truncated COG still opens: the header and the overviews live at the front,
# so gdalinfo succeeds on a half-downloaded file. The size check above is what
# establishes completeness; this establishes that it is the raster we expect.
print "==> verifying"
let info = (do { ^gdalinfo -json $out } | complete | get stdout | from json)
let sz = $info.size
let nd = ($info.bands.0.noDataValue? | default null)
let px = $info.geoTransform.1
print $"    ($sz.0) x ($sz.1) px, ($px) m, nodata ($nd)"
if $sz.0 != 191007 or $sz.1 != 233000 {
    print $"    NOTE: differs from the 191007x233000 seen on 2026-10-01"
}
if $nd != -9999.0 {
    error make {msg: $"nodata is ($nd), expected -9999 — do not build the VRT on this"}
}

# ── Build the VRT ─────────────────────────────────────────────────────────────

if not ($"($DEST)/all.vrt" | path exists) {
    print "==> building all.vrt"
    # One source, and nodata is declared honestly at source — nothing to
    # override, and 0.00 is real ground here, so no -srcnodata. See the header.
    gdal build-vrt [$out] $"($DEST)/all.vrt" --index $"($DEST)/tiles.txt"
}

print $"==> Done -> ($DEST)/all.vrt"
