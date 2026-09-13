#!/usr/bin/env nu

# Fetch AHN5 DTM 0.5 m from PDOK's INSPIRE Atom feed: 1373 GeoTIFF tiles,
# 5 x 6.25 km, EPSG:28992, heights NAP, ~425 GB. Open data, no restrictions.

# AHN's nodata is FLT_MAX, not -9999. Left as-is it reads as a real elevation
# and the hillshade comes out a flat white sheet, so the VRT restamps it.

# RD New looks like a datum hazard (cf. OSGB36, BD72) but is not: PROJ's Helmert
# and the official RDNAPTRANS2018 grid differ by 0.05-0.14 m on the ground,
# measured at Vaalserberg, Amsterdam and Groningen. No grid needed here — do not
# generalise that to other countries.

# Resumable: tiles already on disk are skipped. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-nl.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const FEED = "https://service.pdok.nl/rws/ahn/atom/dtm_05m.xml"
const DEST = "/run/media/martin/2190983A5767510F/AHN/dtm_05m"
const PAR  = 4                      # PDOK is a national service, but stay polite
const TRIES = 4
const EXPECTED = 1373               # tiles in the feed as of 2026-09-12
const NODATA_SRC = "3.4028234663852886e+38"
const NODATA_OUT = "-9999"

mkdir $DEST

# ── Read the feed ─────────────────────────────────────────────────────────────

print "==> Fetching the Atom feed"
let feed = (do { ^curl -sS --fail --location --max-time 120 $FEED } | complete)
if $feed.exit_code != 0 {
    error make {msg: $"could not fetch ($FEED): ($feed.stderr)"}
}

let urls = (
    $feed.stdout
      | parse -r '<link[^>]*href="(?<u>[^"]+\.tif)"'
      | get u
      | uniq
)

print $"==> ($urls | length) tile URLs in the feed"

# A feed that suddenly returns far fewer tiles means AHN republished under a new
# layout — stop rather than quietly build a VRT covering half the country, which
# is the failure that cost a re-render on Belgium.
if ($urls | length) < $EXPECTED {
    error make {msg: $"feed lists ($urls | length) tiles, expected at least ($EXPECTED) — check whether AHN changed its layout before continuing"}
}

# ── Fetch ─────────────────────────────────────────────────────────────────────

let todo = ($urls | where {|u| not ($"($DEST)/($u | path basename)" | path exists) })
print $"==> ($todo | length) pending, ($urls | length) - ($todo | length) already on disk"

$todo | par-each -t $PAR {|u|
    let name = ($u | path basename)
    let out  = $"($DEST)/($name)"
    let part = $"($out).part"

    mut ok = false
    for attempt in 1..$TRIES {
        rm -f $part
        # curl exits 18 on a truncated transfer, so exit 0 already means the
        # whole Content-Length arrived. gdalinfo then confirms it is a readable
        # raster rather than an error page with a .tif name.
        let dl = (do { ^curl -sS --fail --location --retry 2 --retry-delay 5 --connect-timeout 30 --max-time 3600 -o $part $u } | complete)
        if $dl.exit_code == 0 and (do { ^gdalinfo $part } | complete).exit_code == 0 {
            mv $part $out
            $ok = true
            break
        }
        sleep 3sec
    }

    if not $ok { print $"  FAILED ($name)" }
}

# ── Verify and build the national VRT ─────────────────────────────────────────

let have = (ls $"($DEST)/*.tif" | length)
print $"==> ($have) / ($urls | length) tiles present"

if $have != ($urls | length) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
} else {
    print "==> Building all.vrt"
    ls $"($DEST)/*.tif" | get name | str join "\n" | save -f $"($DEST)/tiles.txt"
    # Restamp FLT_MAX as -9999 so every downstream script sees the sentinel it
    # expects; -vrtnodata alone would not, because the sources declare FLT_MAX.
    (gdalbuildvrt -srcnodata $NODATA_SRC -vrtnodata $NODATA_OUT
      -input_file_list $"($DEST)/tiles.txt" $"($DEST)/all.vrt")
    print $"==> Done -> ($DEST)/all.vrt"
}
