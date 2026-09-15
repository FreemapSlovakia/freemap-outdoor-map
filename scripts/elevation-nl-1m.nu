#!/usr/bin/env nu

# Make a 1 m copy of AHN5 for fm6's elevation API. The 0.5 m original stays
# local as the shading source; 1 m is enough for point lookups and is a quarter
# of the bytes to ship and store.

# gdalwarp, not gdal_translate: the source declares nodata as FLT_MAX, and only
# an explicit -srcnodata/-dstnodata pair both averages around it and writes the
# -9999 the rest of the pipeline expects.

# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/elevation-nl-1m.nu

const SRC = "/run/media/martin/2190983A5767510F/AHN/dtm_05m"
const DST = "/run/media/martin/2190983A5767510F/AHN/dtm_1m"
const NODATA_SRC = "3.4028234663852886e+38"
const NODATA_OUT = "-9999"
const PAR = 8

mkdir $DST

let src = (ls $"($SRC)/*.tif" | get name | where {|f| ($f | path basename) != "all.vrt" })
print $"==> ($src | length) source tiles"

let todo = ($src | where {|f| not ($"($DST)/($f | path basename)" | path exists) })
print $"==> ($todo | length) pending"

$todo | par-each -t $PAR {|f|
    let name = ($f | path basename)
    let out  = $"($DST)/($name)"
    let tmp  = $"($out).tmp.tif"
    rm -f $tmp
    # One line: nu mis-parses external args split across lines inside `do {}`.
    let r = (do { ^gdalwarp -q -tr 1 1 -r average -tap -srcnodata $NODATA_SRC -dstnodata $NODATA_OUT -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS $f $tmp } | complete)
    if $r.exit_code == 0 and (do { ^gdalinfo $tmp } | complete).exit_code == 0 {
        mv $tmp $out
    } else {
        rm -f $tmp
        print $"  FAILED ($name)"
    }
}

let have = (ls $"($DST)/*.tif" | length)
print $"==> ($have) / ($src | length) tiles"

if $have != ($src | length) {
    print "==> INCOMPLETE — re-run; VRT not built"
} else {
    print "==> Building all.vrt"
    ls $"($DST)/*.tif" | get name | str join "\n" | save -f $"($DST)/tiles.txt"
    (gdalbuildvrt -vrtnodata $NODATA_OUT -input_file_list $"($DST)/tiles.txt" $"($DST)/all.vrt")
    print $"==> Done -> ($DST)/all.vrt"
}
