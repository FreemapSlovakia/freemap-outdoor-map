# The contour pipeline: smoothed DEM tiles -> one raster -> gdal_contour -> GPKG.
#
# Input is the smooth2m/ directory the matching shading script emits. Contours
# MUST come from the smoothed DEM: contouring raw lidar gives spaghetti, every
# furrow a closed loop. Luxembourg measured 46103 features unsmoothed against
# 30995 smoothed over identical terrain.
#
# Config record — see any scripts/contours-*.nu for a worked example:
#
#   code          short tag, used only for the scratch file-list name
#   data_dir      working directory; the VRT and the offset partials live here
#   src_dir       the shading script's smooth2m/
#   vrt           mosaic over src_dir
#   dem_tif       consolidated single raster
#   gpkg          output; its stem also names the offset partials
#   table         layer name inside the GPKG
#   height_col    attribute carrying the elevation
#   nodata        the sentinel in the source tiles
#   interval      contour interval, metres
#   off_interval  interval of ONE pass; equal to `interval` means a single pass
#   parallel_off  concurrent gdal_contour passes
#   cachemax_mb   GDAL_CACHEMAX PER PROCESS, so it has to be divided by
#                 parallel_off when the passes actually run concurrently. A
#                 single-pass country should take the whole budget
#   epsg          source CRS; only used for the splitter hint printed at the end

use gdal.nu *

# Why consolidate first: gdal_contour over a many-thousand-tile VRT is
# pathologically slow — scattered reads, tiles reopened per scanline. Merging
# into one contiguous raster costs a single sequential pass and the contour then
# reads it straight through.
def consolidate [cfg: record]: nothing -> nothing {
    if ($cfg.dem_tif | path exists) {
        print $"==> ($cfg.dem_tif) exists — reusing"
        return
    }
    print $"==> Consolidating DEM -> ($cfg.dem_tif) — one full pass"
    let tmp = $"($cfg.dem_tif).tmp"
    rm -f $tmp
    (gdal_translate
      --config GDAL_CACHEMAX 16384
      -of GTiff
      -a_nodata $cfg.nodata
      -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES
      -co NUM_THREADS=ALL_CPUS -co BIGTIFF=YES
      $cfg.vrt $tmp)
    mv $tmp $cfg.dem_tif
}

# Offset passes exist because Bayern (33 Gpx, ~290 levels) was OOM-killed in a
# single gdal_contour run: each pass carries a fraction of the levels and the
# union is identical. off_interval == interval is one ordinary pass.
def contour-passes [cfg: record]: nothing -> nothing {
    let stem = ($cfg.gpkg | path parse | get stem)

    # Integer division on purpose: `/` yields a float and `0..0.0` is not
    # iterable in nu, which breaks outright when off_interval == interval.
    let offsets = (0..(($cfg.off_interval // $cfg.interval) - 1) | each {|k| $k * $cfg.interval })
    print $"==> Generating contours in ($offsets | length) offset passes \(-i ($cfg.off_interval), -off ($offsets | str join ', ')\)"

    $offsets | par-each -t $cfg.parallel_off {|off|
        let part = $"($cfg.data_dir)/($stem)_off($off).gpkg"
        if ($part | path exists) {
            print $"  skip offset ($off) — already done"
        } else {
            print $"  start offset ($off)"
            (nice -n 10 gdal_contour
              --config GDAL_CACHEMAX $cfg.cachemax_mb
              -f GPKG
              -nln $cfg.table
              -i $cfg.off_interval
              -off $off
              -a $cfg.height_col
              -snodata $cfg.nodata
              -lco SPATIAL_INDEX=NO
              $cfg.dem_tif $"($part).tmp")
            mv $"($part).tmp" $part
            print $"  done  offset ($off)"
        }
    }

    # Every partial must exist before merging, or the country silently loses a
    # fraction of its contour levels — the same class of failure as merging a
    # partial shading mosaic.
    let missing = ($offsets | where {|off| not ($"($cfg.data_dir)/($stem)_off($off).gpkg" | path exists) })
    if ($missing | is-not-empty) {
        error make {msg: $"offsets ($missing | str join ', ') produced no output — re-run; finished offsets are skipped"}
    }

    # Merge with `enumerate` rather than `first`/`skip`. Both of those take
    # `oneof<table, binary, list<any>>` as pipeline input, and on this list they
    # blew up with "can't convert float to oneof<table, binary, list<any>>"
    # after all ten passes had already succeeded (2026-09-08) — an hour of
    # contouring saved only by the partials still being on disk. `enumerate`
    # turns the list into a table up front and the first/rest split becomes a
    # plain index test, so there is no pipeline-input coercion left to fail.
    print "==> Merging partials"
    let tmp = $"($cfg.gpkg).tmp"
    rm -f $tmp
    for it in ($offsets | enumerate) {
        let part = $"($cfg.data_dir)/($stem)_off($it.item).gpkg"
        if $it.index == 0 {
            print $"  base   offset ($it.item)"
            cp $part $tmp
        } else {
            print $"  append offset ($it.item)"
            ogr2ogr -update -append -nln $cfg.table $tmp $part
        }
    }
    mv $tmp $cfg.gpkg
}

export def run [cfg: record]: nothing -> nothing {
    if (not ($cfg.src_dir | path exists)) or ((glob $"($cfg.src_dir)/*.tif" | length) == 0) {
        error make {msg: $"($cfg.src_dir) is empty — run the matching shading script first \(it emits the smoothed tiles\)"}
    }

    mkdir $cfg.data_dir
    cd $cfg.data_dir

    if ($cfg.vrt | path exists) {
        print $"==> ($cfg.vrt) exists — reusing"
    } else {
        print "==> Building VRT from the smoothed tiles"
        let tiles = (glob $"($cfg.src_dir)/*.tif")
        print $"  ($tiles | length) tiles"
        build-vrt $tiles $cfg.vrt --extra [-vrtnodata $cfg.nodata] --index $"_idx_($cfg.code)_cont"
    }

    consolidate $cfg

    if ($cfg.gpkg | path exists) {
        print $"==> ($cfg.gpkg) already exists — delete it to re-generate; skipping"
        return
    }

    contour-passes $cfg

    print $"==> Done -> ($cfg.gpkg). Partials left in ($cfg.data_dir) — delete once verified."
    print $"    Next: splitter-rs \(--source-epsg ($cfg.epsg | str replace 'EPSG:' '')\); it creates the table and index itself."
}
