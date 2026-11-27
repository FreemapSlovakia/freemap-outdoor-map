#!/usr/bin/env nu

# Download Latvia's open airborne laser scanning (LGIA, cycle 1) and keep it as
# LAZ. Port of download-ro.nu / download-is.nu.
#
# 65917 tiles of 1 km, CC BY 4.0, no authentication, from a plain list of URLs:
#
#     https://s3.storage.pub.lvdc.gov.lv/lgia-opendata/las/LGIA_OpenData_las_saites.txt
#
# THE DELIVERY IS UNCOMPRESSED .las AND IT IS ENORMOUS. Measured over 90 tiles
# spread across the country: mean 227 MB, median 216 MB, range 29-659 MB, so the
# raw set is ~15 TB. laszip gets 4.9x on it, which is still ~3 TB, so EVERY TILE
# IS COMPRESSED ON ARRIVAL AND THE .las DELETED. Nothing else fits on any one
# drive here.
#
# LAS CARRIES NO CRC, so there is no cheap integrity check like the Romanian
# zips have. laszip reading every point IS the check: a truncated or corrupt
# file fails to compress. A tile counts as done only once its .laz exists.
#
# CRS IS NOT IN THE FILES. Every tile has VLRs: 0 -- no GeoKeyDirectoryTag, no
# projection of any kind -- and LGIA's own pages disagree, the English one
# saying LKS-92 TM and the Latvian one LKS-2020 TM. It is EPSG:3059 (LKS-92 TM):
# a 1 m DTM rendered on that assumption matched LGIA's own published 20 m
# product to about 0.2 m over 40 sampled points, which it could not do if the
# projection were wrong. Downstream must assert 3059; nothing here can read it.
#
# DO NOT DATE TILES FROM GPS TIME. The header's time encoding is inconsistent:
# of 24 tiles sampled, 8 store GPS week seconds, and 2 more declare Adjusted
# Standard GPS Time in the global encoding while storing week seconds anyway.
# Adding 1e9 to a week-seconds value lands on 2011-09-14, which is exactly 1e9
# seconds after the GPS epoch and looks like a plausible flight date. Acquisition
# is 2015-2019 where it can be read at all; nothing in this delivery is from
# cycle 2, which began in 2022 and is not published here.
#
# ORDERED BY MAP SHEET so finished sheets are contiguous: the 1 m render needs a
# sheet plus a one-tile collar present at once, and a sheet can be rendered and
# its LAZ deleted while the rest still downloads.
#
# STOPS BEFORE IT FILLS THE DISK. 3 TB does not fit, so the run exits cleanly
# once free space reaches $FLOOR rather than wedging the filesystem. Re-run it
# after freeing space; every verified tile is skipped.
#
# PARALLEL 4, measured: 34 MB/s on one stream, 80 MB/s on four, and back to
# 34 MB/s on eight. Four is the knee, not a guess.
#
# TRANSFER COMPRESSION IS NOT AVAILABLE, so the 15 TB crosses the wire whole.
# The store answers Accept-Encoding with no Content-Encoding and the same byte
# count either way -- measured at 41.9 MB for a 40 MiB range both plain and with
# --compressed. It is an S3-style object store serving stored bytes verbatim.
#
# Run via:
#   nu ~/fm/freemap-outdoor-map/scripts/download-lv.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const ROOT     = "/run/media/martin/14TB/lv"
const LAZ_DIR  = "/run/media/martin/14TB/lv/laz"
const INDEX    = "/run/media/martin/14TB/lv/LGIA_OpenData_las_saites.txt"
const LIST_URL = "https://s3.storage.pub.lvdc.gov.lv/lgia-opendata/las/LGIA_OpenData_las_saites.txt"
const LASZIP   = "/home/martin/fm/LAStools/bin/laszip"
const PARALLEL = 4
const RETRIES  = 3
const FLOOR_GB = 60        # stop with this much left, so the disk never fills

# ── Helpers ───────────────────────────────────────────────────────────────────

def free-gb []: nothing -> int {
    let out = (^df -B1G --output=avail $ROOT | lines | last | str trim)
    ($out | into int)
}

# One tile: fetch whole, compress, drop the .las. A partial is discarded, never
# resumed -- a killed curl can leave the right byte count around a gap, and
# `curl -C -` would then append good data after it.
def get-tile [url: string, sheet: string, stem: string]: nothing -> string {
    let dir = $"($LAZ_DIR)/($sheet)"
    let laz = $"($dir)/($stem).laz"
    let las = $"($dir)/($stem).las"

    if ($laz | path exists) and ((ls -l $laz | get 0.size | into int) > 0) {
        return "skip"
    }

    mkdir $dir
    for attempt in 1..$RETRIES {
        rm -f $las $laz
        let got = (
            do { (curl -sS -L --fail --retry 4 --retry-delay 5 --retry-all-errors
                    --max-time 3600 -o $las $url) } | complete
        )
        if $got.exit_code != 0 { continue }

        # laszip walks every point, so this doubles as the integrity check the
        # format does not provide.
        let zipped = (do { ^$LASZIP -i $las -o $laz } | complete)
        rm -f $las
        if $zipped.exit_code == 0 and ($laz | path exists) and ((ls -l $laz | get 0.size | into int) > 0) {
            return "ok"
        }
        rm -f $laz
    }
    rm -f $las
    "fail"
}

# ── Index ─────────────────────────────────────────────────────────────────────

mkdir $ROOT
mkdir $LAZ_DIR

if not ($INDEX | path exists) {
    print $"==> fetching the tile list to ($INDEX)"
    (curl -sS -L --fail --max-time 300 -o $INDEX $LIST_URL)
}

let urls = (open --raw $INDEX | lines | each {|l| $l | str trim } | where {|l| ($l | str length) > 0 })
print $"==> ($urls | length) tiles in the list"

# Sheet is the directory component of the URL; stem is the file name without
# its extension. Sorted so sheets complete in order and can be rendered early.
let tiles = (
    $urls
      | each {|u|
            let parts = ($u | split row "/")
            {url: $u, sheet: ($parts | get (($parts | length) - 2)), stem: ($parts | last | str replace ".las" "")}
        }
      | sort-by sheet stem
)
print $"==> ($tiles | group-by sheet | columns | length) map sheets"

let have = (
    glob $"($LAZ_DIR)/*/*.laz" | each {|f| $f | path parse | get stem }
)
let pending = ($tiles | where {|t| $t.stem not-in $have })
print $"==> ($have | length) tiles already stored, ($pending | length) to fetch"
print $"==> ($ROOT): ($pending | length) tiles still to come, (free-gb) GB free, stopping at ($FLOOR_GB) GB"

if ($pending | is-empty) {
    print "==> Nothing to do."
    exit 0
}

# ── Fetch ─────────────────────────────────────────────────────────────────────

mut done_n = 0
mut failed = []
let total = ($pending | length)

let by_sheet = ($pending | group-by sheet)
mut stopped = false

for sheet in ($by_sheet | columns | sort) {
    if $stopped { continue }
    if (free-gb) < $FLOOR_GB {
        print $"==> STOPPING: only (free-gb) GB left on ($ROOT), floor is ($FLOOR_GB) GB."
        print "    Free some space and re-run; stored tiles are skipped."
        $stopped = true
        continue
    }

    let st = ($by_sheet | get $sheet)
    let res = ($st | par-each -t $PARALLEL {|t| {stem: $t.stem, r: (get-tile $t.url $t.sheet $t.stem)} })
    $done_n = $done_n + ($st | length)
    $failed = ($failed | append ($res | where r == "fail" | get stem))

    let pct = ($done_n * 100 / $total)
    print $"    sheet ($sheet): ($st | length) tiles  ->  ($done_n)/($total) \(($pct)%\)  (free-gb) GB free  failures ($failed | length)"
}

print ""
print $"==> stored: (glob $"($LAZ_DIR)/*/*.laz" | length) tiles"
if ($failed | is-not-empty) {
    print $"==> ($failed | length) tiles failed; re-run to retry them:"
    $failed | first 20 | each {|s| print $"    ($s)" }
}
print $"==> ($ROOT) now has (free-gb) GB free"
