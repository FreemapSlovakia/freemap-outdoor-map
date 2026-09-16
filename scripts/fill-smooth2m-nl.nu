#!/usr/bin/env nu

# One-off repair: fill the voids in the 2 m contour tiles the Netherlands run
# emitted before shading-nl.nu was corrected.
#
# shading-nl.nu step 3 read $smooth (unfilled) instead of $dem (filled), so every
# building, tree clump and ditch stayed a hole and gdal_contour broke a line at
# each one — visible as gaps while the hillshade beside them looked continuous.
# The script is fixed; this repairs the already-written tiles without a 20 h
# re-render.
#
# -md 3 at 2 m is a 6 m reach, matching the 5 m the hillshade got from -md 10 at
# 0.5 m. Filling at 2 m rather than 0.5 m is coarser, but a building-sized void is
# still building-sized and the contour interval is 5 m.
#
# Writes to a new directory; the originals are left alone.
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/fill-smooth2m-nl.nu

const SRC = "/mnt/osm/nl/smooth2m"
const DST = "/mnt/osm/nl/smooth2m-filled"
const PAR = 6
const MD  = 3

mkdir $DST

let src = (ls $"($SRC)/*.tif" | get name)
print $"==> ($src | length) tiles"

let todo = ($src | where {|f| not ($"($DST)/($f | path basename)" | path exists) })
print $"==> ($todo | length) pending"

$todo | par-each -t $PAR {|f|
    let name = ($f | path basename)
    let out  = $"($DST)/($name)"
    # NOT *.tmp.tif — that still matches the `*.tif` glob contours-nl.nu feeds to
    # gdalbuildvrt, so a tile left behind by an interrupted run would be
    # contoured as if it were finished, and the completeness check below would
    # count it and never print the INCOMPLETE warning.
    let tmp  = $"($out).part"
    rm -f $tmp
    let r = (do { ^gdal_fillnodata.py -md $MD -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES $f $tmp } | complete)
    if $r.exit_code == 0 and (do { ^gdalinfo $tmp } | complete).exit_code == 0 {
        mv $tmp $out
    } else {
        rm -f $tmp
        print $"  FAILED ($name)"
    }
}

let have = (ls $"($DST)/*.tif" | length)
print $"==> ($have) / ($src | length) tiles filled"
if $have != ($src | length) {
    print "==> INCOMPLETE — re-run to pick up the stragglers"
}
