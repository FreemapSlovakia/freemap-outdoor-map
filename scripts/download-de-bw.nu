#!/usr/bin/env nu

# Fetch the Baden-Württemberg DGM1 (1 m digital terrain model) — 9371 downloads
# of 2x2 km, each holding four 1x1 km sub-tiles, 37484 rasters in all.
#
# Source: LGL Baden-Württemberg Open GeoData Portal, product DGM1.
#   https://opengeodata.lgl-bw.de/#/(sidenav:product/dgm1)
#   EPSG:25832 (ETRS89 / UTM 32N), heights DE_DHHN2016_NH, accuracy 0.15 m,
#   from laser scanning at 8 points/m2 flown in rolling coverage since 2016.
#
# LICENCE dl-de/by-2-0, with the credit prescribed verbatim by the portal:
#   "Datenquelle: LGL, www.lgl-bw.de, dl-de/by-2-0". Commercial and
#   non-commercial use are both permitted.
#
# THE PORTAL CAPS SELECTION AT 10 TILES; THE URLS DO NOT. The download page is
#   an Angular app that refuses more than ten tiles per request, which would
#   mean 938 manual rounds. The tile URLs are plain and constructible — the same
#   situation as Saxony's Nextcloud share — so the cap is a UI limit, not an
#   access control. Nothing here needs a login or a token.
#
#   The portal also ships an owsproxy username and password in
#   /assets/environment/config.json. It is for their WMS proxy, these downloads
#   do not need it, and it must not be copied into this repository.
#
# THE TILE GRID SITS ON ODD EASTINGS AND EVEN NORTHINGS. dgm1_32_525_5388
#   exists; 524_5388 and 525_5387 do not. Sachsen-Anhalt's grid is even/even, so
#   a generator ported from it returns 404 for every single tile. The index
#   below was read out of the server once (2026-09-23) by probing the 14964
#   candidates in the state bounding box; 9371 answered 200.
#
# THE PAYLOAD IS XYZ ASCII, NOT GeoTIFF. Each zip holds four .xyz of a million
#   "x y z" lines, 29 MB apiece — 1.03 TB across the state if extracted all at
#   once, against 69 GB once converted. So each archive is extracted, converted
#   and swept in turn, and the zip is dropped as soon as its rasters exist.
#
# COORDINATES ARE CELL CENTRES — 397000.50, not 397000.00 — and GDAL's XYZ
#   driver reads that convention correctly on its own, placing the origin at
#   397000.00. Do NOT add the +0.5 shift that Thüringen's delivery needed: there
#   the georeference was inferred from corner coordinates and came out half a
#   cell off, which is the opposite mistake.
#
# NODATA IS 0.00, AND THE DELIVERY SAYS SO NOWHERE. Every .xyz line carries a z,
#   so cells outside coverage are written as 0.00 rather than omitted or flagged.
#   Converting with -a_nodata -9999 therefore marks nothing, and a border tile
#   becomes half real terrain and half sea level: dgm1_32_581_5276 reads
#   min 0.00, max 1021.99, mean 446 — a 1 km cliff along the state boundary that
#   would have been hillshaded and contoured as if it were ground.
#
#   0 IS SAFE TO MASK HERE, MEASURED. Across 400 random rasters not one pixel
#   falls in (0, 80) m and the lowest real elevation is 89.10 m, the Rhine
#   graben being the state's floor. This is the same overloading of 0 that
#   Poland's delivery has, and it is caught the same way.
#
# EACH SUB-TILE CARRIES ITS OWN VINTAGE IN ITS NAME, e.g.
#   dgm1_32_397_5322_1_bw_2019.tif, so the year cannot be predicted and
#   resumability tests a glob rather than an exact filename. A .csv sidecar per
#   sub-tile gives the survey date and accuracy; it is kept.
#
# aria2c NEEDS A BROWSER USER-AGENT. With its default one every request fails.
#
# Resumable: a 2 km tile whose four rasters are present is skipped without a
# request. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-de-bw.nu

use lib/gdal.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const BASE = "https://opengeodata.lgl-bw.de/data/dgm"
const MOUNT = "/run/media/martin/2190983A5767510F"   # assert the drive, not the dataset dir
const DEST = "/run/media/martin/2190983A5767510F/DGM1/Baden-Wuerttemberg"
const EPSG = "EPSG:25832"
const UA   = "Mozilla/5.0 (X11; Linux x86_64)"
# Converters draining the archive buffer. Measured on 24 cores while download
# and conversion were still in lockstep: 6 gave 79 rasters/min and 10 gave 66,
# so the drive, not the CPU, is the limit and oversubscribing it costs
# throughput. Raise only alongside a measurement.
const PAR = 8

# Concurrent downloads. Independent of PAR now that the two run side by side;
# the link reached 40 MB/s in bursts, which is well ahead of what the
# converters can consume, so this only has to keep the buffer from emptying.
const DL_PAR = 8
const EXPECTED = 9371

# Tile index, run-length encoded as "northing:easting-ranges", step 2 on both
# axes. Eastings are ODD, northings EVEN — see the header.
const RLE = "5264:399-403
5266:397-409,417-431
5268:395-409,417-431,447-461,545
5270:393-437,447-461,467,539-547
5272:393-463,467-469,539-557
5274:391-469,537-559
5276:389-471,489-493,511-513,533-561,579-581
5278:387-459,463-471,489-497,509-515,523-573,579-583
5280:387-455,469,473-483,487-499,503-515,519-585
5282:387-455,473-585
5284:387-459,473-585
5286:389-459,477-583
5288:389-459,471,475-583
5290:389-583
5292:389-581
5294:389-585
5296:389-585
5298:391-583
5300:391-581
5302:391-581
5304:391-581
5306:393-583
5308:393-583
5310:393-581
5312:395-581
5314:395-583
5316:395-583
5318:393-583
5320:393-583
5322:393-585
5324:393-585
5326:393-583
5328:393-583
5330:393-583
5332:395-581
5334:395-581
5336:395-581
5338:397-579
5340:399-579
5342:401-579,583
5344:401-579
5346:401-579
5348:401-579
5350:401-577
5352:405-575
5354:405-575
5356:405-573
5358:405-573
5360:405-575
5362:405-575
5364:407-577
5366:407-579,583-585
5368:407-587
5370:407-591
5372:409-591
5374:411-597
5376:411-595
5378:411-597
5380:411-595
5382:411-595
5384:411-597
5386:413-595
5388:413-593,599
5390:417-593,599-607
5392:419-609
5394:421-609
5396:421-609
5398:423-607
5400:423-603
5402:427-605
5404:427-605
5406:433-605
5408:433-605
5410:433-605
5412:435-605
5414:435-605
5416:435-605
5418:437-605
5420:439-605
5422:439-605
5424:443-603
5426:445-603
5428:447-601
5430:447-597
5432:447-595
5434:449-591
5436:451-591
5438:451-591
5440:453-589
5442:453-591
5444:453-591
5446:453-589
5448:453-585
5450:455-583
5452:455-581
5454:455-581
5456:457-583
5458:459-583
5460:461-583
5462:459-583
5464:459-581
5466:461-581
5468:461-581
5470:461-583
5472:463-583
5474:463-487,491-583
5476:459-487,491-581
5478:459-491,495-581
5480:459-491,495-581
5482:459-491,495-565,571-579
5484:457-487,497-499,503-565,573-581
5486:457-479,485,503-567,575-579
5488:457-465,471-477,505-567,575-577
5490:457-463,471-477,505-565
5492:457-461,469-477,515-565
5494:469-477,517-563
5496:471-477,519-561
5498:519-561
5500:521,525-559
5502:527-559
5504:525-559
5506:523-559
5508:521-545,549-551,555-557
5510:521-545
5512:521-545
5514:525-535,539-545"

gdal assert-mounted $MOUNT
gdal require-proj $EPSG "download-de-bw.nu"

mkdir $"($DEST)/zips"

# ── Expand the index ──────────────────────────────────────────────────────────

let tiles = (
    $RLE | lines | each {|row|
        let parts = ($row | split row ":")
        let n = ($parts.0 | into int)
        $parts.1 | split row "," | each {|r|
            let ab = ($r | split row "-")
            let a = ($ab.0 | into int)
            let b = (if ($ab | length) > 1 { $ab.1 | into int } else { $a })
            (seq $a 2 $b) | each {|e| {e: $e, n: $n} }
        } | flatten
    } | flatten
)

print $"==> ($tiles | length) tiles in index \(expected ($EXPECTED)\)"
if ($tiles | length) != $EXPECTED {
    error make {msg: $"index expands to ($tiles | length), expected ($EXPECTED) — RLE is corrupt"}
}

# ── Fetch, convert, sweep ─────────────────────────────────────────────────────

# COMPLETENESS IS A MARKER PER ARCHIVE, NOT A COUNT OF RASTERS. A 2 km archive
# does NOT reliably hold four 1 km sub-tiles: the state boundary is not aligned
# to the kilometre grid, so edge archives hold one, two or three, and seven of
# them hold none at all (just a licence PDF and a metadata note). Measured over
# the full state: 9371 archives yielded 36667 rasters, not 37484, and 401
# archives were short. Requiring four would refetch those 401 on every run and
# — worse — leave `missing` permanently non-empty, so the VRT would never be
# built.
#
# One empty file per processed archive, listed once, is also far cheaper than
# globbing 36k rasters on the DGM drive: it is NTFS over ntfs-3g, so every stat
# is a FUSE round trip.
def done-set [dest: string]: nothing -> record {
    do { ^find $"($dest)/.done" -maxdepth 1 -type f -printf "%f\n" } | complete
      | get stdout | lines
      | reduce --fold {} {|f, acc| $acc | upsert $f true }
}


mkdir $"($DEST)/.done"
let done0 = (done-set $DEST)
let pending = ($tiles | where {|t| not ($done0 | get -o $"dgm1_32_($t.e)_($t.n)_2_bw" | default false) })
print $"==> ($pending | length) pending, ($tiles | length) total"

# DOWNLOAD AND CONVERSION RUN CONCURRENTLY, NOT IN LOCKSTEP. Fetching an
# archive and then converting it in the same worker leaves the link idle for
# as long as the conversion takes, and conversion is the slower of the two —
# measured, the network sat at nothing between short 40 MB/s bursts. So one
# aria2c is handed the whole list and left to fill a buffer of archives while
# a separate pool drains it.
#
# aria2c marks work in progress with a sibling .aria2 control file and removes
# it on completion, so "a .zip with no .aria2 beside it" is the ready signal.
if ($pending | is-not-empty) {
    let zipdir = $"($DEST)/zips"
    rm -f $"($zipdir)/.dl-done"

    # Archives left by an interrupted run are truncated and have lost their
    # control file, so they can neither be resumed nor trusted. Drop them here
    # and they are simply fetched again.
    for z in (glob $"($zipdir)/*.zip") {
        if not ($"($z).aria2" | path exists) {
            if (do { ^unzip -tqq $z } | complete).exit_code != 0 { rm -f $z }
        }
    }

    ($pending | each {|t| $"($BASE)/dgm1_32_($t.e)_($t.n)_2_bw.zip" } | str join "\n")
      | save -f $"($zipdir)/urls.txt"

    # Detached, so the conversion loop below starts draining immediately.
    (do {
        ^bash -c $"nohup aria2c -i '($zipdir)/urls.txt' -j ($DL_PAR) -x4 -s4 --continue=true --auto-file-renaming=false --user-agent '($UA)' --console-log-level=error --summary-interval=0 -d '($zipdir)' > '($zipdir)/aria2.log' 2>&1; touch '($zipdir)/.dl-done' &"
    } | complete) | ignore
    print $"==> downloader started \(($DL_PAR) concurrent\), draining with ($PAR) converters"

    loop {
        let ready = (
            glob $"($zipdir)/*.zip"
              | where {|z| not ($"($z).aria2" | path exists) }
        )

        if ($ready | is-empty) {
            if ($"($zipdir)/.dl-done" | path exists) { break }
            sleep 3sec
            continue
        }

        $ready | par-each -t $PAR {|zip|
            let stem = ($zip | path basename | str replace ".zip" "")

            if (do { ^unzip -tqq $zip } | complete).exit_code != 0 {
                print $"  CORRUPT ($stem) — dropping"
                rm -f $zip
                return
            }

            # EXTRACT TO RAM, NOT TO THE DRIVE. The .xyz are 29 MB each and
            # exist only to be converted, so unpacking them beside the rasters
            # would push 1.06 TB through the DGM drive and read it all
            # back — which measured slower than the download itself.
            let work = $"/dev/shm/bw_($stem)"
            rm -rf $work
            mkdir $work
            (do { ^unzip -oqq $zip -d $work } | complete) | ignore
            for f in (glob $"($work)/*/*") { mv -f $f $work }

            for xyz in (glob $"($work)/*.xyz") {
                let stem2 = ($xyz | path basename | str replace ".xyz" "")
                let tif = $"($DEST)/($stem2).tif"
                if not ($tif | path exists) {
                    # PREDICTOR=3 is fine here: this raster is read by GDAL
                    # only. shading-de-bw.nu rewrites its window DEM with
                    # PREDICTOR=1 for feature-preserving-smoothing, which
                    # ignores the tag.
                    let c = (do {
                        ^gdal_translate -q -of GTiff -a_srs $EPSG -a_nodata 0 -co COMPRESS=LZW -co PREDICTOR=3 -co TILED=YES $xyz $"($tif).tmp"
                    } | complete)
                    if $c.exit_code == 0 and ($"($tif).tmp" | path exists) {
                        mv $"($tif).tmp" $tif
                    } else {
                        rm -f $"($tif).tmp"
                        print $"  FAILED convert ($stem2)"
                        continue
                    }
                }
            }

            # The .csv sidecars carry the per-sub-tile survey date and accuracy.
            for c in (glob $"($work)/*.csv") { mv -f $c $DEST }

            rm -rf $work
            rm -f $zip
            touch $"($DEST)/.done/($stem)"
        } | ignore
    }
}

# ── Verify and build the state VRT ────────────────────────────────────────────

let tifs = (do { ^find $DEST -maxdepth 1 -name "*.tif" } | complete | get stdout | lines)
let done1 = (done-set $DEST)
let missing = ($tiles | where {|t| not ($done1 | get -o $"dgm1_32_($t.e)_($t.n)_2_bw" | default false) })

# 36667 rasters over the full state, not 9371 * 4 — see done-set.
print $"==> ($tifs | length) rasters from ($tiles | length) archives; ($missing | length) archives unprocessed"

if ($missing | is-not-empty) {
    print $"==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
    print $"    first few: ($missing | first 10 | each {|t| $'($t.e)_($t.n)'} | str join ', ')"
} else {
    rm -rf $"($DEST)/zips"
    if not ($"($DEST)/all.vrt" | path exists) {
        print "==> building all.vrt"
        # -srcnodata 0 is load-bearing: the delivery writes out-of-coverage as
        # 0.00, so without it the state border is a cliff to sea level.
        (gdal build-vrt $tifs $"($DEST)/all.vrt"
           --extra [-srcnodata 0 -vrtnodata -9999]
           --index $"($DEST)/tiles.txt")
    }
    print $"==> Done -> ($DEST)/all.vrt"
}
