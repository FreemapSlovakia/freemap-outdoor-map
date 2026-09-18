#!/usr/bin/env nu

# Fetch the Saxon DGM1 (1 m digital terrain model) — 4981 tiles of 2x2 km.
#
# Source: GeoSN (Staatsbetrieb Geobasisinformation und Vermessung Sachsen),
#   product DGM1_TIFF_2km "Digitales Geländemodell 1m". GeoTIFF, EPSG:25833
#   (ETRS89 / UTM 33N), heights DHHN2016. Free open data.
#
# WHY A HARDCODED TILE TABLE. The batch-download page at
#   https://www.geodaten.sachsen.de/batch-download-4719.html is an Angular form:
#   pick Landkreis + product, and it renders ~5000 <a> elements pointing at a
#   Nextcloud public share. There is no index file and no directory listing —
#   /public.php/dav/files/<token>/ answers 404 without a filename. So the tile
#   list was read out of that page once (2026-09-09) and is embedded below in
#   run-length form. Re-derive it from the page if Saxony ever extends coverage.
#
# THE TOKEN IS NOT A CREDENTIAL. JCcXyifaNdLDnxZ is the public share id that the
#   open-data page hands to every visitor; no login, no agreement. Same status as
#   England's hardcoded site key.
#
# THE SERVER THROTTLES, AND IT THROTTLES AS 404 — NOT 429. Observed 2026-09-09:
#   the first ~2450 tiles came down cleanly at PAR=6, then every request began
#   returning a Sabre\DAV NotFound, INCLUDING tiles that had downloaded
#   successfully an hour earlier. Re-reading the batch page returned the very
#   same token and the very same 4981 URLs, so this is not share expiry — it is
#   the host declining to serve us any more for a while.
#
#   The trap is that a 404 reads as "this tile does not exist", so a retry loop
#   will burn through its attempts, mark good tiles as permanently FAILED, and
#   leave a mosaic with holes. If failures suddenly appear across the board
#   rather than scattered, STOP and wait rather than re-running: the run is
#   resumable and completed tiles are skipped, so nothing is lost by pausing.
#   Lower PAR and add a delay before resuming.
#
# TILE NAMING: dgm1_{EEEEE}_{NNNN}_2_sn_tiff.zip, where EEEEE is the UTM33
#   easting in km WITH the zone prefix (33278 = zone 33, E 278000) and NNNN is
#   the northing in km (5590 = N 5 590 000). Step 2 km in both axes.
#
# Resumable: a tile whose .tif already exists is skipped, and partial downloads
# resume with curl -C -. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/download-sn.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const BASE  = "https://geocloud.landesvermessung.sachsen.de/public.php/dav/files/JCcXyifaNdLDnxZ"
const DEST  = "/run/media/martin/2190983A5767510F/DGM1/Sachsen"
const PAR   = 2            # concurrent downloads — 6 tripped the throttle above
const TRIES = 4
const EXPECTED = 4981

# Tile index, run-length encoded as "northing:easting-ranges" (step 2).
const RLE = "5560:33306-33310
5562:33304-33310
5564:33304-33308
5566:33304-33310
5568:33302-33310
5570:33302-33312
5572:33300-33314
5574:33300-33314
5576:33292-33316
5578:33288-33318
5580:33284-33320
5582:33284-33322
5584:33284-33326,33336,33352-33354
5586:33282-33338,33350-33356
5588:33280-33358
5590:33278-33358
5592:33278-33358
5594:33280-33364,33370-33372
5596:33282-33372
5598:33282-33372
5600:33278-33374
5602:33278-33374,33378
5604:33278-33380
5606:33282-33382,33388-33392
5608:33282-33290,33294-33394
5610:33284-33288,33296-33394
5612:33286-33288,33296-33396
5614:33306-33396
5616:33308-33396
5618:33306-33402,33406-33408,33416-33418
5620:33304-33422
5622:33304-33422
5624:33304-33422
5626:33304-33426
5628:33306-33434,33480-33484
5630:33304-33438,33476-33486
5632:33304-33444,33472-33486
5634:33308-33446,33472-33486
5636:33314-33454,33472-33488
5638:33320-33456,33474-33490
5640:33320-33458,33468-33490
5642:33324-33458,33468-33492
5644:33332-33454,33470-33492
5646:33332-33452,33470-33494
5648:33332-33452,33470-33494
5650:33328-33448,33458,33464-33494
5652:33326-33450,33456-33496
5654:33324-33496
5656:33324-33498
5658:33322-33498
5660:33314-33498
5662:33308-33498
5664:33304-33498
5666:33304-33500
5668:33302-33500
5670:33302-33500
5672:33302-33500
5674:33302-33500
5676:33302-33502
5678:33302-33502
5680:33302-33502
5682:33302-33502
5684:33300-33500
5686:33300-33498
5688:33300-33498
5690:33302-33498
5692:33302-33408,33412-33498
5694:33302-33396,33426-33496
5696:33302-33374,33378-33396,33432-33498
5698:33302-33374,33382-33392,33432-33498
5700:33302-33374,33382,33386-33390,33432-33496
5702:33302-33374,33432-33496
5704:33300-33374,33434-33492
5706:33302-33374,33436-33488
5708:33304-33374,33436-33482
5710:33304-33374,33438-33446,33456-33480
5712:33304-33374,33462-33472,33478-33480
5714:33306-33372,33468-33470,33478-33480
5716:33308,33312-33372,33478-33480
5718:33314-33372
5720:33322-33370
5722:33332-33366
5724:33338-33362
5726:33338-33342,33346-33354,33358-33360
5728:33348-33350"

# ── Expand the tile table ─────────────────────────────────────────────────────

def expand []: nothing -> table {
    $RLE | lines | each {|row|
        let parts = ($row | split row ":")
        let n = ($parts | get 0 | into int)
        $parts | get 1 | split row "," | each {|span|
            let ends = ($span | split row "-")
            let a = ($ends | get 0 | into int)
            let b = (if ($ends | length) > 1 { $ends | get 1 | into int } else { $a })
            # step 2 km; `..` is inclusive, so filter to the even offsets
            ($a..$b) | where {|e| ($e - $a) mod 2 == 0 } | each {|e| {e: $e, n: $n} }
        } | flatten
    } | flatten
}

let tiles = (expand)

# A miscount means the embedded table drifted from the portal — better to stop
# than to silently build a state VRT with holes in it, which is exactly the
# failure mode that cost a re-render on Belgium.
if ($tiles | length) != $EXPECTED {
    error make {msg: $"tile table expands to ($tiles | length) tiles, expected ($EXPECTED) — re-read the batch-download page"}
}

mkdir $DEST
print $"==> ($tiles | length) tiles -> ($DEST)"

# ── Fetch ─────────────────────────────────────────────────────────────────────

let todo = ($tiles | where {|t| not ($"($DEST)/dgm1_($t.e)_($t.n)_2_sn.tif" | path exists) })
print $"==> ($todo | length) pending, ($tiles | length) - ($todo | length) already done"

$todo | par-each -t $PAR {|t|
    let stem = $"dgm1_($t.e)_($t.n)_2_sn"
    let zip  = $"($DEST)/($stem).zip"
    let tif  = $"($DEST)/($stem).tif"

    if ($tif | path exists) { return }

    # No -C - resume: tiles are ~10 MB, so restarting a failed attempt is cheaper
    # than reasoning about a half-written file, and a resumed-but-corrupt zip
    # never heals. Each attempt starts clean. --fail so an HTTP error is an exit
    # code rather than an HTML error page written into the .zip.
    # NOTE the _tiff suffix: the archive is dgm1_..._2_sn_TIFF.zip but the raster
    # inside it is dgm1_..._2_sn.tif. One stem cannot serve both names.
    let url = $"($BASE)/($stem)_tiff.zip"
    mut ok = false
    for attempt in 1..$TRIES {
        rm -f $zip
        let dl = (do { ^curl -sS --fail --location --retry 2 --retry-delay 3 --connect-timeout 30 --max-time 600 -o $zip $url } | complete)
        if $dl.exit_code == 0 and (do { ^unzip -tqq $zip } | complete).exit_code == 0 {
            $ok = true
            break
        }
        sleep 2sec
    }

    if not $ok {
        print $"  FAILED ($stem)"
        return
    }

    # Each zip holds the .tif plus a metadata sidecar; keep only the raster.
    (do { ^unzip -oqq $zip -d $DEST } | complete) | ignore
    rm -f $zip
    let got = (ls $"($DEST)/($stem)*.tif" | get name)
    if ($got | is-empty) { print $"  NO TIF IN ZIP ($stem)" }
}

# ── Verify and build the state VRT ────────────────────────────────────────────

let have = (ls $"($DEST)/*.tif" | length)
print $"==> ($have) / ($tiles | length) tifs present"

if $have != ($tiles | length) {
    print "==> INCOMPLETE — re-run to pick up the stragglers; VRT not built"
} else {
    print "==> Building all.vrt"
    ls $"($DEST)/*.tif" | get name | str join "\n" | save -f $"($DEST)/tiles.txt"
    (gdalbuildvrt -input_file_list $"($DEST)/tiles.txt" $"($DEST)/all.vrt")
    print $"==> Done -> ($DEST)/all.vrt"
}
