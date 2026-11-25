#!/usr/bin/env nu

# Download Iceland's national elevation model (ÍslandsDEM v1.0, 10 m).
#
# THE BULK DOWNLOAD IS NOT ADVERTISED BY ANY OGC SERVICE. Every endpoint in the
# dataset's metadata serves rendered or derived rasters: gis.natt.is/mapcache/*
# is image/png, and the shared GeoServer behind gis.lmi.is and gis.natt.is
# publishes only TRI, TPI, islandsdem_skuggar_50m and islandsdem_hillshade_10m
# as coverages. The last of those is A HILLSHADE STORED AS Float32, so a dtype
# check passes it as elevation — do not take it. The real rasters are on a plain
# directory index linked from the GeoNetwork record:
#
#     https://ftp.natt.is/gisdata/raster/
#
# WHICH FILE. `zmasl` means "z metres above sea level" and is the only elevation
# product; `daylight` and `shade` are pictures, `aspect` and `slope_*` derived.
# ISN93 (EPSG:3057) over the ISN2016 twin (EPSG:8088): both are published, and
# 3057 has the better PROJ support.
#
# 10 m IS THE BEST OPEN RESOLUTION, not the best that exists. ÍslandsDEM is also
# published at 2 m, but only as WMTS pictures — no coverage, no file. Ask
# Náttúrufræðistofnun for the 2 m as GeoTIFF; CC BY 4.0 is already granted, so
# that is a distribution request and not a licensing one.
#
# Licence: CC BY 4.0 — the GeoNetwork record states "no limitations to public
# access" and "Attribution 4.0 International (CC BY 4.0)". Attribute
# Náttúrufræðistofnun (which absorbed Landmælingar Íslands).
#
# NOT LIDAR THROUGHOUT, AND THAT MATTERS FOR GLACIERS. v1.0 is largely
# ArcticDEM stereo tiles co-registered into one mosaic, so it is a SURFACE
# model. Iceland is ~2% forested, so over land that is as good as terrain — but
# ~11% of the country is under ice that moves metres a year, and a multi-date
# mosaic gives glacier contours that are both wrong and discontinuous between
# tiles. The date raster fetched below carries the acquisition date PER PIXEL,
# which is how to see where that happens instead of guessing.
#
# Both download-ro.nu lessons apply even to two files:
#   1. NEVER RESUME A PARTIAL. A killed run can leave a file of the right size
#      whose middle is garbage; `curl -C -` then appends good data after the
#      gap. Partials are discarded and re-fetched whole.
#   2. VERIFY CONTENT, NOT SIZE — AND STRICTLY. A file passes only if gdalinfo
#      emits NO ERROR. "A Checksum= line was printed" is not enough: with only
#      some blocks undecodable GDAL prints ERRORs *and* a checksum.
#
# Resumable: a verified file is never re-fetched. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-is.nu

use lib/gdal.nu

const BASE = "https://ftp.natt.is/gisdata/raster"

# The date raster is only published on the isn2016 grid. It is read for
# inspection, never mosaicked, so the CRS mismatch with the DEM does not matter.
const FILES = [
    [name                                                     purpose];
    ["IslandsDEMv1.0_10x10m_isn93_zmasl.tif"                   "DEM, 10 m, EPSG:3057"]
    ["IslandsDEMv1_lidar_dates_yyyymmdd_10x10m_isn2016.tif"    "acquisition date per pixel"]
]

let DRIVE = (gdal find-drive)
let DEST  = $"($DRIVE)/is"
print $"==> drive: ($DRIVE)"
mkdir $DEST

# Strict content check — see header lesson 2.
def raster-ok [f: string]: nothing -> bool {
    if not ($f | path exists) { return false }
    let res = (do { ^gdalinfo -checksum $f } | complete)
    if $res.exit_code != 0 { return false }
    let out = $"($res.stdout)($res.stderr)"
    if ($out | str contains "ERROR") { return false }
    ($out | str contains "Checksum=")
}

for f in $FILES {
    let dst = $"($DEST)/($f.name)"

    if (raster-ok $dst) {
        print $"==> ($f.name): already present and verified — skipping"
        continue
    }

    print $"==> fetching ($f.name)  \(($f.purpose)\)"
    mut ok = false
    for attempt in 1..3 {
        rm -f $dst
        let res = (
            do {
                (curl -sS -L --fail --retry 5 --retry-delay 5 --retry-all-errors
                   --max-time 7200 -o $dst $"($BASE)/($f.name)")
            } | complete
        )
        if $res.exit_code != 0 {
            print $"    curl exit ($res.exit_code) on attempt ($attempt)"
            continue
        }
        if (raster-ok $dst) { $ok = true; break }
        print $"    gdalinfo rejected the file on attempt ($attempt) — discarding"
    }
    if not $ok {
        rm -f $dst
        error make {msg: $"($f.name) could not be fetched and verified from ($BASE)"}
    }
    print $"    ok  ($dst)"
}

# ── Report what arrived ───────────────────────────────────────────────────────

print ""
print "==> Delivery:"
for f in $FILES {
    let dst = $"($DEST)/($f.name)"
    let sz = (ls -l $dst | get 0.size)
    let r = (gdal raster-res $dst)
    let s = (gdal raster-size $dst)
    print $"  ($f.name)"
    print $"    ($sz)  ($s.w) x ($s.h) px  pixel ($r.x) x ($r.y) m"
}

let dem = $"($DEST)/(($FILES | first).name)"
let mm = (gdal band-minmax $dem)
print ""
print $"==> DEM range: ($mm.min) .. ($mm.max) m"
print "    Iceland's summit is Hvannadalshnúkur at 2110 m; a max far above that"
print "    means the band is not metres above sea level and nothing downstream"
print "    should be trusted."
print ""
print $"==> Next: shading-is.nu reads ($dem) directly as `src` — it is one"
print "    national raster, so there is no VRT to build."
