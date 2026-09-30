#!/usr/bin/env nu

# Fetch Danmarks Højdemodel / Terræn — 668 blocks of 10x10 km holding 1 km
# tiles at 0.4 m, 795 GB on the wire, about 80 GB once resampled to 1 m.
#
# Source: Klimadatastyrelsen, via Dataforsyningen's FTPS server.
#   ftps://ftp.dataforsyningen.dk:990/dhm_danmarks_hoejdemodel/DTM/
#   GeoTIFF, Float32, DEFLATE, 0.4 m, EPSG:25832, nodata -9999.
#
# LICENCE CC BY 4.0. Credit "Klimadatastyrelsen" — the terms page lists
#   Klimadatastyrelsen, GeoDanmark and Geodatastyrelsen under plain CC BY 4.0,
#   with other agencies on separate terms that do not apply to this dataset.
#
# IT NEEDS AN ACCOUNT, AND THE TOKEN IS NOT IT. Dataforsyningen issues API
#   tokens for its web services (WCS/WMS/WMTS), but the FTP server
#   authenticates with the account's own username and password. Credentials
#   live in ~/.netrc, mode 600:
#
#       machine ftp.dataforsyningen.dk
#       login <user>
#       password <password>
#
#   so nothing appears in this repository, in `ps`, or in a shell history.
#   Register at https://dataforsyningen.dk — free, and the download is not
#   rate-limited in any way we met.
#
# PORT 990, NOT 21. The portal's "copy FTP link" gives ftps://…/DTM without a
#   port; 21 is refused outright and only implicit FTPS on 990 answers. A wrong
#   password there returns 530 rather than anything descriptive.
#
# RESAMPLED TO 1 m ON THE WAY IN, WHICH IS NOT A COMPROMISE. The source is
#   0.4 m and the map renders at z16, which is 2.39 m a pixel — six times
#   coarser than the source and twice as coarse as the 1 m kept here. Keeping
#   0.4 m would cost about 500 GB to serve a resolution nothing reads. The
#   averaging is `gdalwarp -tr 1 1 -r average`, verified to preserve the tile
#   origin exactly: a 2500x2500 tile at 684000/6050000 becomes 1000x1000 at the
#   same corner.
#
# EVERY TILE SHIPS AN .md5 BESIDE IT, which is checked before conversion. A
#   block whose archive is intact can still hold a truncated member.
#
# 0.00 IS EVERYWHERE AND IS PROBABLY SEA, BUT VERIFY BEFORE RENDERING. The
#   first coastal tile sampled is 93% exact zeros. Denmark's lowest land is
#   about -7 m in the Lammefjord polder, so a scan comparing against that floor
#   will separate sea from real ground — do not assume either way. Four German
#   states in this repository each used 0.00 differently.
#
# A BLOCK IS MARKED DONE ONLY WHEN EVERY TILE IN IT CONVERTED. The marker is
#   what makes a re-run skip a block, and the completeness test at the end
#   counts markers, so marking a block whose tiles partly failed would hide the
#   gap twice over: the re-run would not revisit it and the VRT would be built
#   over holes. The count is checked against the archive's own directory rather
#   than against what was extracted, so a truncated archive cannot satisfy it.
#
# Resumable: a tile already converted is not refetched, and a block is skipped
# once all its tiles are present. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-dk.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const HOST  = "ftps://ftp.dataforsyningen.dk:990"
const REMOTE = "/dhm_danmarks_hoejdemodel/DTM"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Denmark"
const STAGE = "/mnt/osm/dk-stage"                    # block archives, on NVMe
const EPSG  = "EPSG:25832"                           # ETRS89 / UTM zone 32N

# Six streams measured 90 MB/s aggregate; the server did not throttle, and the
# converters keep up because each block is only a few hundred tiles.
const DL_PAR = 6
const PAR    = 8

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-dk.nu"

if not ("~/.netrc" | path expand | path exists) {
    error make {msg: "~/.netrc not found — see the header for the required machine/login/password entry"}
}

mkdir $DEST
mkdir $STAGE

# ── List the blocks ───────────────────────────────────────────────────────────

print "==> listing the remote directory"
let listing = (
    do { ^curl -sS --ssl-reqd --netrc --list-only $"($HOST)($REMOTE)/" } | complete | get stdout
      | lines | where {|l| $l | str ends-with ".zip" }
)
print $"==> ($listing | length) blocks offered"
if ($listing | length) < 100 {
    error make {msg: "the listing came back far too short — check ~/.netrc and the server"}
}

# ── Fetch and convert, block by block ─────────────────────────────────────────

# A block is done when its marker exists. Counting tiles per block would mean
# opening the archive again on every run, and the archives are 1 GB apiece.
mkdir $"($DEST)/.done"
let done = (
    do { ^find $"($DEST)/.done" -maxdepth 1 -type f -printf "%f\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|f, acc| $acc | upsert $f true }
)

let todo = ($listing | where {|z| not ($done | get -o $z | default false) })
print $"==> ($todo | length) blocks to fetch, ($done | columns | length) already done"

mut n = 0

# Sliced by hand because `chunks` fails here: nushell 0.114 raises "can't
# convert list<string> to oneof<list<list<any>>, list<binary>>" at the chunks
# call, and deleting the loop that consumes its result makes the same call on
# the same list succeed — so the operand is fine and the fault is in how the
# two compile together.
let total = ($todo | length)
mut i = 0

while $i < $total {
    let chunk = ($todo | skip $i | first $DL_PAR)
    $i += $DL_PAR

    # Fetch this group in parallel, then convert it; the two do not overlap
    # because a block is 1 GB and holding many in flight would fill the stage.
    $chunk | par-each -t $DL_PAR {|z|
        let zip = $"($STAGE)/($z)"
        # A staged archive may be a truncated leftover: suspending the machine
        # kills the transfer and curl leaves what it had. Test before trusting
        # it, so it is refetched in this pass rather than failing its CRC at
        # conversion time and being deferred to another pass over all 668.
        if ($zip | path exists) and (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
            print $"  STALE ($z) — truncated leftover, refetching"
            rm -f $zip
        }
        if not ($zip | path exists) {
            (do { ^curl -sS --ssl-reqd --netrc --max-time 1800 -o $zip $"($HOST)($REMOTE)/($z)" } | complete) | ignore
        }
    } | ignore

    for z in $chunk {
        let zip = $"($STAGE)/($z)"
        if not ($zip | path exists) {
            print $"  MISSING ($z) — will retry on the next run"
            continue
        }
        if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
            print $"  CORRUPT ($z) — dropped, will refetch"
            rm -f $zip
            continue
        }

        let work = $"($STAGE)/x_($z | str replace '.zip' '')"
        rm -rf $work
        mkdir $work
        (do { ^unzip -joqq $zip -d $work } | complete) | ignore

        let listed = (
            do { ^unzip -Z1 $zip } | complete | get stdout | lines
              | where {|e| $e | str ends-with ".tif" } | length
        )
        let tifs = (glob $"($work)/*.tif")

        let ok = ($tifs | par-each -t $PAR {|t|
            let stem = ($t | path basename | str replace ".tif" "")
            let out = $"($DEST)/($stem).tif"
            if ($out | path exists) {
                true
            } else {
                # The .md5 beside each tile guards against a truncated member
                # inside an otherwise intact archive.
                let md5f = $"($work)/($stem).md5"
                let sound = (if ($md5f | path exists) {
                    let want = (open --raw $md5f | str trim | split row " " | first)
                    let got = (do { ^md5sum $t } | complete | get stdout | split row " " | first)
                    if $want != $got { print $"  MD5 MISMATCH ($stem)"; false } else { true }
                } else { true })

                if not $sound {
                    false
                } else {
                    let c = (do {
                        ^gdalwarp -q -tr 1 1 -r average -of GTiff -co COMPRESS=DEFLATE -co PREDICTOR=3 -co ZLEVEL=6 -co TILED=YES $t $"($out).tmp"
                    } | complete)
                    if $c.exit_code == 0 and ($"($out).tmp" | path exists) {
                        mv -f $"($out).tmp" $out
                        true
                    } else {
                        rm -f $"($out).tmp"
                        print $"  FAILED convert ($stem)"
                        false
                    }
                }
            }
        } | where {|x| $x } | length)

        rm -rf $work

        # See the header: the marker is only earned by a complete block.
        if $ok == $listed {
            rm -f $zip
            touch $"($DEST)/.done/($z)"
            $n += 1
        } else {
            print $"  ($z): ($ok) of ($listed) tiles converted — left unmarked, archive kept"
        }
    }
    print $"  ($n) / ($total) blocks converted"
}

# ── Verify and build the VRT ──────────────────────────────────────────────────

let missing = ($listing | where {|z| not ($"($DEST)/.done/($z)" | path exists) })
let have = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
print $"==> ($have | length) tiles present; ($missing | length) blocks unprocessed"

if ($missing | is-not-empty) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    first few: ($missing | first 5 | str join ', ')"
} else {
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # nodata is declared honestly at source and survives the warp.
        gdal build-vrt $have $"($DEST)/all.vrt" --index $"($DEST)/tiles.txt"
    }
    rm -rf $STAGE
    print $"==> Done -> ($DEST)/all.vrt"
}
