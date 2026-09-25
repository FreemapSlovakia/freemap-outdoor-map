#!/usr/bin/env nu

# Fetch the Rheinland-Pfalz DGM1 (1 m digital terrain model) — 21160 tiles of
# 1x1 km, 30.8 GB.
#
# Source: LVermGeo Rheinland-Pfalz, open geodata via GeoShop.
#   https://geoshop.rlp.de/opendata-dgm1.html
#   GeoTIFF, Float32, LZW, nodata -9999, 1 m, vintages 2022-2025.
#
# LICENCE dl-de/by-2-0, with the credit prescribed verbatim:
#   "©GeoBasis-DE / LVermGeoRP <Jahr des Datenbezugs>, dl-de/by-2-0,
#   www.lvermgeo.rlp.de"
#
# THE WHOLE STATE IS ONE METALINK, which is why this script is short. The
#   product page drives itself from a JSON config that names
#   dgm1_tif_07.meta4 — a standard Metalink 4 listing every file with its size,
#   its URL and a sha-256. So there is no tile index to scrape and no portal to
#   reverse-engineer; the hashes are carried through to aria2c per URL, so
#   every raster is still verified as it lands. Nothing else here has been this
#   clean.
#
#   The metalink is regenerated upstream (see the `published` field), so it is
#   fetched fresh rather than pinned. 42320 entries = 21160 .tif plus a .tfw
#   each; only the rasters are requested, and the sweep below clears any
#   sidecars an earlier run left behind.
#
# THE CRS IS COMPOUND — ETRS89 / UTM 32N + DHHN2016 height — which no other
#   state here uses, and `gdalsrsinfo -o epsg` prints nothing for it. That is
#   fine in itself: a warp to EPSG:3857 was tested and gives the right extent.
#
#   BUT 78 OF THE 21160 RASTERS CARRY THE PLAIN PROJCRS INSTEAD, and
#   gdalbuildvrt will not mix the two. Its complaint is worse than useless —
#   "expected ETRS89 / UTM zone 32N, got ETRS89 / UTM zone 32N" — because the
#   names match and only the vertical part differs. It drops those 78 into a
#   warning behind its progress bar, exactly as it drops Hessen's untagged
#   rasters. `gdal build-vrt` catches it because it checks every offered input
#   back out of the VRT; without that guard the state would simply be short 78
#   tiles. stamp_crs.py normalises them onto the majority CRS below, keeping
#   the vertical datum rather than flattening everything to EPSG:25832.
#
# NODATA IS AN HONEST -9999 and the sample tile holds no zeros, so unlike
#   Baden-Württemberg there is no -srcnodata and no fringe prefilter. Verify
#   that on the full state before rendering rather than trusting one tile —
#   Hessen declared -9999 too and still hid a zero row in 17 rasters.
#
# Resumable: a raster already on disk is not re-fetched, so a re-run costs one
# directory listing. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-rp.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const META  = "https://geobasis-rlp.de/data/dgm1/current/meta4/dgm1_tif_07.meta4"
const MOUNT = "/run/media/martin/2190983A5767510F"
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Rheinland-Pfalz"
const STAGE = "/mnt/osm/rp-stage"                  # metalink + aria2 session, on NVMe
const EPSG  = "EPSG:25832"
const UA    = "Mozilla/5.0 (X11; Linux x86_64)"
const PAR   = 24                                   # concurrent files; small files, so many at once
const CONN  = 1                                    # connections per file; -x4 on 1.5 MB is setup cost for nothing
const STAMP = "/home/martin/fm/freemap-outdoor-map/scripts/stamp_crs.py"
const SYS_PYTHON = "/usr/bin/python3"
const EXPECTED_TIF = 21160

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-rp.nu"

mkdir $DEST
mkdir $STAGE

# ── Fetch the metalink ────────────────────────────────────────────────────────

let m4 = $"($STAGE)/dgm1_tif_07.meta4"
print "==> fetching the metalink"
(do { ^curl -sS -L --fail --max-time 300 -A $UA -o $m4 $META } | complete) | ignore
if not ($m4 | path exists) {
    error make {msg: $"could not fetch ($META)"}
}

let raw = (open --raw $m4)
let names = ($raw | parse -r '<file name="([^"]+)"' | get capture0)
let tifs = ($names | where {|n| $n | str ends-with ".tif" })
print $"==> metalink lists ($names | length) entries, ($tifs | length) rasters \(expected ($EXPECTED_TIF)\)"
if ($tifs | length) != $EXPECTED_TIF {
    print $"    NOTE: raster count differs from the ($EXPECTED_TIF) seen on 2026-09-25 — coverage may have grown"
}

# ── Download ──────────────────────────────────────────────────────────────────

# DO NOT HAND aria2c THE METALINK DIRECTLY. With 42320 entries and --continue it
# re-examines the whole queue before fetching anything, stat-ing every entry on
# a spinning NTFS-over-ntfs-3g volume where every stat is a FUSE round trip:
# measured 51 rasters/min that way, and 1/min at -j 32. Working out what
# is missing here — one directory listing — and feeding aria2c only that gives
# 170/min. The sha-256 is carried across as a per-URL `checksum=` option, so
# nothing is lost by not using the metalink's own queue.
#
# Small files, so ONE connection each and many at once: -x 4 on a 1.5 MB file
# is setup cost for nothing. The server paces small requests at roughly
# 200/min regardless of destination, so the drive is not the limit.
print "==> working out what is missing"
let present = (
    do { ^find $DEST -maxdepth 1 -name "*.tif" -printf "%f %s\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|l, acc|
          let p = ($l | split row " ")
          if ($p | length) > 1 { $acc | upsert $p.0 ($p.1 | into int) } else { $acc }
      }
)

let want = (
    $raw
      | parse -r '<file name="([^"]+)">\s*<size>(\d+)</size>\s*<hash type="sha-256">([0-9a-f]+)</hash>\s*<url>([^<]+)</url>'
      | where {|r| $r.capture0 | str ends-with ".tif" }
)
print $"==> ($want | length) rasters in the metalink"

# PRESENCE, NOT SIZE. The obvious test is "size differs from the metalink", and
# it works exactly once: normalising the CRS below rewrites 78 headers and their
# sizes no longer match, so every later run would re-fetch those 78 and aria2c
# would exit 13 because the files are already there. Integrity is aria2c's job
# at download time, from the sha-256 carried in the input file.
let todo = ($want | where {|r| ($present | get -o $r.capture0 | default 0) == 0 })
print $"==> ($todo | length) to fetch"

if ($todo | is-not-empty) {
    let input = $"($STAGE)/input.txt"
    ($todo | each {|r| $"($r.capture3)\n  out=($r.capture0)\n  checksum=sha-256=($r.capture2)" }
       | str join "\n") | save -f $input

    let dl = (do {
        ^aria2c -i $input -j $PAR -x $CONN -s $CONN --auto-file-renaming=false --user-agent $UA --connect-timeout=20 --timeout=60 --max-tries=5 --retry-wait=5 --console-log-level=error --summary-interval=60 -d $DEST
    } | complete)

    if $dl.exit_code != 0 {
        print $"==> aria2c exited ($dl.exit_code) — re-run to resume"
    }
}

# ── Sweep the sidecars, verify, build the state VRT ───────────────────────────

# The .tfw add nothing: the GeoTIFF carries origin and pixel size itself,
# verified with the sidecar removed.
let tfw = (do { ^find $DEST -maxdepth 1 -name "*.tfw" } | complete | get stdout | lines)
if ($tfw | is-not-empty) {
    print $"==> removing ($tfw | length) redundant .tfw sidecars"
    $tfw | each {|f| rm -f $f } | ignore
}

let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
let missing = ($tifs | where {|n| not ($"($DEST)/($n)" | path exists) })
print $"==> ($have | length) rasters present; ($missing | length) missing"

if ($missing | is-not-empty) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    first few: ($missing | first 5 | str join ', ')"
} else {
    # 78 rasters carry the plain PROJCRS where the rest are COMPOUNDCRS, and
    # gdalbuildvrt silently drops the minority — see the header.
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
