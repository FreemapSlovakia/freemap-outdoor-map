#!/usr/bin/env nu

# ── Configuration ─────────────────────────────────────────────────────────────

const ZOOM     = 16   # target zoom level; determines output pixel size
const PARALLEL = 24   # number of tiles processed in parallel
const OVERLAP  = 6    # half of retile overlap; used for hillshading context
const CROP     = 3    # pixels cropped from each edge after hillshading; OVERLAP - CROP leaves margin for warp alignment
const TMPDIR   = "/dev/shm"  # ramdisk for intermediate files; change to "." to use local disk

# ── Helpers ───────────────────────────────────────────────────────────────────

def has-data [file: string]: nothing -> bool {
    let band = gdalinfo -json -mm $file err> /dev/null | from json | get bands | first
    ($band | get -o computedMin | is-not-empty)
}

# Weighted multi-directional hillshade blend formula for one RGB band.
# wa/wb/wc are hex weights for azimuth directions a/b/c.
def band-calc [wa: string, wb: string, wc: string]: nothing -> string {
    let ea  = "0.8 * (255 - A)"
    let eb  = "0.7 * (255 - B)"
    let ec  = "1.0 * (255 - C)"
    let num = $"($ea) * ($wa) + ($eb) * ($wb) + ($ec) * ($wc)"
    let den = $"0.01 + ($ea) + ($eb) + ($ec)"
    "((" + $num + ") / (" + $den + ") - 128.0) + 128.0"
}

# Alpha channel: inverse of "all directions dark simultaneously".
def alpha-calc []: nothing -> string {
    let ea = "0.8 * (255 - A)"
    let eb = "0.7 * (255 - B)"
    let ec = "1.0 * (255 - C)"
    "255.0 - 255.0 * ((1.0 - " + $ea + " / 255.0) * (1.0 - " + $eb + " / 255.0) * (1.0 - " + $ec + " / 255.0))"
}

# Process one smooth tile into a warped RGBA shaded-relief GeoTIFF.
# Output goes to tiles/<stem>/ ; uses a .tmp dir for crash-safe resumability.
def process-tile [src: string, tr: string]: nothing -> nothing {
    let stem = $src | path basename | path parse | get stem
    let d    = $"($TMPDIR)/shading_($stem)"
    let out  = $"tiles/($stem).tif"
    print $"  tile ($stem): hillshade"

    rm -rf $d
    mkdir $d

    let co      = [-co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS]
    let co_big  = [...$co -co BIGTIFF=YES]
    let co_calc = [--co=COMPRESS=ZSTD --co=PREDICTOR=2 --co=TILED=YES --co=NUM_THREADS=ALL_CPUS --co=BIGTIFF=YES]

    # Three hillshades at different azimuths (run on smooth tile with overlap so edges have real neighbours)
    gdaldem hillshade $src $"($d)/_a.tif" -az -120 -igor -compute_edges ...$co o> /dev/null
    gdaldem hillshade $src $"($d)/_b.tif" -az  60  -igor -compute_edges ...$co o> /dev/null
    gdaldem hillshade $src $"($d)/_c.tif" -az -45  -igor -compute_edges ...$co o> /dev/null

    # Crop overlap from hillshades — discards edge pixels degraded by smoothing
    let info = gdalinfo -json $src | from json
    let w = $info.size.0
    let h = $info.size.1
    for name in [a b c] {
        let raw = $"($d)/_($name)_raw.tif"
        mv $"($d)/_($name).tif" $raw
        gdal_translate -srcwin $CROP $CROP ($w - 2 * $CROP) ($h - 2 * $CROP) ...$co $raw $"($d)/_($name).tif" o> /dev/null
        rm $raw
    }

    # Warp each to EPSG:3857 at zoom-level pixel size
    print $"  tile ($stem): warp"
    for name in [a b c] {
        gdalwarp -t_srs EPSG:3857 -tr $tr $tr -tap -r cubic -dstnodata none -of GTiff ...$co_big -multi -wo NUM_THREADS=ALL_CPUS -wo INIT_DEST=0 $"($d)/_($name).tif" $"($d)/($name)-warped.tif" o> /dev/null
    }

    # Compute RGBA bands from the three warped hillshades
    print $"  tile ($stem): bands"
    let inputs = [-A $"($d)/a-warped.tif" -B $"($d)/b-warped.tif" -C $"($d)/c-warped.tif"]

    #                       [a]    [b]    [c]
    let r_calc = band-calc "0x20" "0xFF" "0x00"
    let g_calc = band-calc "0x30" "0xEE" "0x00"
    let b_calc = band-calc "0x60" "0x00" "0x00"
    let a_calc = alpha-calc

    gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/R.tif" $"--calc=($r_calc)" o> /dev/null
    gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/G.tif" $"--calc=($g_calc)" o> /dev/null
    gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/B.tif" $"--calc=($b_calc)" o> /dev/null
    gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/A.tif" $"--calc=($a_calc)" o> /dev/null

    # Stack RGBA into a VRT with the alpha as internal mask
    print $"  tile ($stem): stack + translate"
    let vrt = $"($d)/stack.vrt"
    gdalbuildvrt -separate $vrt $"($d)/R.tif" $"($d)/G.tif" $"($d)/B.tif" $"($d)/A.tif" o> /dev/null
    gdal_edit.py -colorinterp_1 red -colorinterp_2 green -colorinterp_3 blue $vrt o> /dev/null
    sed -i '/<NoDataValue>/d; /<NODATA>/d; /<SrcRect/d; /<DstRect/d; s/ComplexSource/SimpleSource/g' $vrt
    sed -i 's|</VRTDataset>|<MaskBand><VRTRasterBand dataType="Byte"><SimpleSource><SourceFilename relativeToVRT="1">a-warped.tif</SourceFilename><SourceBand>1</SourceBand></SimpleSource></VRTRasterBand></MaskBand></VRTDataset>|' $vrt

    # Translate to final GeoTIFF
    gdal_translate --config GDAL_TIFF_INTERNAL_MASK YES -of GTiff ...$co_big $vrt $"($d)/final.tif" o> /dev/null

    mv $"($d)/final.tif" $out
    rm -rf $d
    print $"  tile ($stem): done"
}

# ── Pipeline ──────────────────────────────────────────────────────────────────

let pi = (1 | math arctan) * 4
let tr = ($pi * 2 * 6378137 / 256 / (2 ** $ZOOM) | into string)
print $"ZOOM=($ZOOM) TR=($tr)"

# # 1. Build VRT from source DTM files
# print "==> Building source VRT"
# glob sweden_dtm/**/*.tif | save -f dtm_index
# gdalbuildvrt -input_file_list dtm_index all.vrt

# # 2. Retile with overlap (overlap is kept through processing to avoid hillshade edge artifacts)
# print "==> Retiling"
# mkdir retiled
# gdal_retile.py all.vrt -ps 1500 1500 -overlap 12 -targetDir retiled -co COMPRESS=ZSTD -co PREDICTOR=1

# # 3. Smooth tiles — resumable, skips existing and all-nodata tiles
# print "==> Smoothing"
# mkdir smooth
# (
#   glob retiled/*.tif
#     | where {|f| not ($"smooth/($f | path basename)" | path exists)}
#     | where {|f| has-data $f}
#     | par-each -t $PARALLEL {|f|
#         let a   = $f | path basename
#         let dst = $"smooth/($a)"
#         print $"  smooth ($a)"
#         whitebox_tools -r=FeaturePreservingSmoothing -v --wd="." --dem=retiled/($a) -o=($"($dst).tmp") --filter=11 --norm_diff=16.0 --num_iter=6
#         mv $"($dst).tmp" $dst
#       }
# )

# 4. Process each smooth tile into shaded relief — resumable
print "==> Processing tiles"
mkdir tiles
(
  glob smooth/*.tif
    | sort
    | where {|f| not ($"tiles/($f | path basename | path parse | get stem).tif" | path exists)}
    # | first 20
    | par-each -t $PARALLEL {|src| process-tile $src $tr}
)

# 5. Merge all tiles and build overviews
print "==> Merging tiles"
glob tiles/*.tif | save -f shading_index
gdalbuildvrt -input_file_list shading_index shading.vrt
sed -i 's|<ColorInterp>Alpha</ColorInterp>|<ColorInterp>Undefined</ColorInterp>|g' shading.vrt
gdal_translate --config GDAL_TIFF_INTERNAL_MASK YES -of GTiff -co TILED=YES -co COMPRESS=ZSTD -co PREDICTOR=2 -co BIGTIFF=YES -co NUM_THREADS=ALL_CPUS shading.vrt shading.tif
rm shading.vrt shading_index
gdal_edit.py -colorinterp_4 alpha shading.tif
print "==> Building overviews"
gdaladdo --config GDAL_TIFF_INTERNAL_MASK YES --config GDAL_CACHEMAX 4096 --config GDAL_NUM_THREADS ALL_CPUS -r cubic shading.tif
print "==> Done"
