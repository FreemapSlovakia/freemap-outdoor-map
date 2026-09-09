#!/usr/bin/env nu

# Render one Saxony window at several zoom levels so the ZOOM constant in
# shading-de-sn.nu can be chosen from evidence instead of from analogy.
# Port of sample-zoom-en.nu; method deliberately unchanged.
#
# This is the measurement Norway's shading-no.nu header records having done on a
# 1.5 km window of Ostmarka, reproduced here as a script so it can be re-run on
# any window. The method is deliberately the same:
#
#   * render the SAME window at each zoom with the SAME pipeline as production
#     (smooth 11/16/6/6 -> three Igor hillshades -> weighted RGBA blend);
#   * upscale each coarser render to the finest zoom's grid;
#   * compare against the native finest render, and report the mean difference,
#     the p95, and the share of pixels off by more than 5 levels out of 255.
#
# WHY THE COMPARISON IS DONE THAT WAY. The question is not "does a finer zoom
# hold more numbers" — it trivially does. It is "does a finer zoom show the
# viewer anything a cheaper one, stretched to the same screen size, does not".
# Upscaling the coarse render to the fine grid is exactly what a map client does
# when it overzooms, so this measures the difference a user could actually see.
#
# BEST CASE, ON PURPOSE. Pick the roughest window available: whatever conclusion
# holds on the most detailed terrain in the country holds everywhere flatter.
# For Saxony that is Sächsische Schweiz, not the Erzgebirge — the Erzgebirge is
# higher (Fichtelberg 1215 m) but is rounded, glacially-smoothed ridge, whereas
# the Elbe sandstone is vertical walls, slots and free-standing towers with metre
# -scale structure. Relief alone is the wrong proxy for what a hillshade shows;
# roughness at the pixel scale is the thing. If the sandstone cannot justify a
# finer zoom, nothing in Saxony can.
#
# The ceiling is the source, not the grid: the DTM is a 1 m grid, and
# feature-preserving-smoothing --filter 11 strips most sub-11 m variation before
# the hillshade is ever computed. So a zoom finer than ~1 m/px is resampling
# detail the pipeline has already removed.
#
# Outputs, in OUT_DIR:
#   z{N}.tif          the RGBA render at each zoom, EPSG:3857
#   z{N}.png          a fixed-extent crop of each, all the same pixel size,
#                     for side-by-side visual comparison
#   report.json       the numbers, plus the extrapolated disk cost per zoom
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/sample-zoom-sn.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_DIR = "/run/media/martin/2190983A5767510F/DGM1/Sachsen"
const OUT_DIR = "/run/media/martin/18TB/sn/zoom-sample"
const NODATA  = "-9999"
const ZOOMS   = [15 16 17]                  # ascending; the last is the reference
# Saxon DGM1 tiles are 2 x 2 km, named dgm1_{E}_{N}_2_sn where E carries the UTM
# zone prefix (33414 = zone 33, E 414000). This one is the Bastei / Schrammsteine
# block of Sächsische Schweiz — sandstone gorges and free-standing towers, the
# roughest ground in the state. If it cannot justify a finer zoom, nothing in
# Saxony can; everything else here is Erzgebirge ridge or lowland.
const TILE    = "dgm1_33414_5650_2_sn"
const WIN_M   = 2000                        # a whole tile
const COLLAR  = 6
const CROP    = 3
const TMPDIR  = "/dev/shm"

# A tight crop for the visual comparison — big enough to read the terrain, small
# enough that the PNGs stay light. Offset from the window origin, in metres.
const CROP_OFF = 600
const CROP_M   = 1200

const SM_FILTER    = 11
const SM_NORM_DIFF = 16
const SM_NUM_ITER  = 6
const SM_MAX_DIFF  = 6

# Saxony's land area, for extrapolating the disk cost of each zoom from the
# measured size of this window.
const ENGLAND_KM2 = 18450

# Latitude span of Saxony, for converting a Web Mercator pixel size into real
# ground metres. Mercator pixels shrink as cos(lat), so the -tr passed to
# gdalwarp is NOT the ground resolution — at 50-52 degN it overstates it by
# roughly a third, and quoting it unqualified would make every zoom look finer
# than it is.
const LAT_MIN = 50.17
const LAT_MAX = 51.69

# ── Helpers (identical formulas to shading-en.nu) ──────────────────────────────

def band-calc [wa: string, wb: string, wc: string]: nothing -> string {
    let ea  = "0.8 * (255 - A)"
    let eb  = "0.7 * (255 - B)"
    let ec  = "1.0 * (255 - C)"
    let num = $"($ea) * ($wa) + ($eb) * ($wb) + ($ec) * ($wc)"
    let den = $"0.01 + ($ea) + ($eb) + ($ec)"
    "((" + $num + ") / (" + $den + ") - 128.0) + 128.0"
}

def alpha-calc []: nothing -> string {
    let ea = "0.8 * (255 - A)"
    let eb = "0.7 * (255 - B)"
    let ec = "1.0 * (255 - C)"
    "255.0 - 255.0 * ((1.0 - " + $ea + " / 255.0) * (1.0 - " + $eb + " / 255.0) * (1.0 - " + $ec + " / 255.0))"
}

# ── Prepare the window: cut, smooth, hillshade. Done ONCE, shared by all zooms ─
# The DEM work is zoom-independent; only the warp differs. Doing it once also
# guarantees the zooms are compared on identical input rather than on two runs
# of a non-deterministic smoother.

mkdir $OUT_DIR
let d = $"($TMPDIR)/en_zoomsample"
rm -rf $d
mkdir $d

let co = [-co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS]

# Two tiny python helpers, written to disk rather than passed with `python3 -c`:
# nu's $"..." interpolation claims parentheses, so inlining python source into an
# interpolated string mangles every call. Plain single-quoted strings here, and
# arguments arrive via argv.
'
import json, sys
import numpy as np
from osgeo import gdal
a = gdal.Open(sys.argv[1]).ReadAsArray().astype("float32")
b = gdal.Open(sys.argv[2]).ReadAsArray().astype("float32")
d = np.abs(a - b)
print(json.dumps({
    "mean": round(float(d.mean()), 3),
    "p95": round(float(np.percentile(d, 95)), 2),
    "max": round(float(d.max()), 1),
    "pct_over_5": round(float((d > 5).mean() * 100), 2),
    "pixels": int(d.size),
}))
' | save -f $"($d)/diff.py"

'
import sys
from pyproj import Transformer
t = Transformer.from_crs("EPSG:25833", "EPSG:3857", always_xy=True)
x0, y0, x1, y1 = (float(v) for v in sys.argv[1:5])
xs, ys = [], []
for x, y in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]:
    a, b = t.transform(x, y)
    xs.append(a); ys.append(b)
print(f"{min(xs)} {min(ys)} {max(xs)} {max(ys)}")
' | save -f $"($d)/reproj.py"

print $"==> Window: ($TILE), ($WIN_M) m"

let src = $"($SRC_DIR)/($TILE).tif"
if not ($src | path exists) {
    error make {msg: $"($src) not on disk — pick a TILE that has been downloaded"}
}

let g = (gdalinfo -json $src | from json | get geoTransform)
let xmin = ($g.0 | into float)
let ymax = ($g.3 | into float)
let xmax = ($xmin + $WIN_M)
let ymin = ($ymax - $WIN_M)
print $"    extent E ($xmin) .. ($xmax)   N ($ymin) .. ($ymax)  \(EPSG:25833\)"

# VRT over ALL tiles, not just the chosen one. England's 5 km tiles left room to
# take the smoothing collar from inside a single tile; Saxony's are 2 km and the
# window here IS the whole tile, so the collar has to come from the neighbours.
# Reading one small window through a 5000-tile VRT costs nothing — GDAL only
# opens the tiles the window actually touches.
ls $"($SRC_DIR)/*.tif" | get name | str join "\n" | save -f $"($d)/srclist.txt"
gdalbuildvrt -vrtnodata $NODATA -input_file_list $"($d)/srclist.txt" $"($d)/src.vrt" o> /dev/null

let win = $"($d)/win.tif"
(gdal_translate -q -of GTiff
  -projwin ($xmin - $COLLAR) ($ymax + $COLLAR) ($xmax + $COLLAR) ($ymin - $COLLAR)
  -co COMPRESS=DEFLATE -co PREDICTOR=1 -co TILED=YES
  $"($d)/src.vrt" $win o> /dev/null)

print "==> Smoothing (11/16/6/6)"
let smooth = $"($d)/smooth.tif"
(feature-preserving-smoothing --dem $win -o $smooth
  --filter $SM_FILTER --norm_diff $SM_NORM_DIFF
  --num_iter $SM_NUM_ITER --max_diff $SM_MAX_DIFF)

let dem = $"($d)/dem.tif"
gdal_fillnodata.py -md 5 $smooth $dem o> /dev/null err> /dev/null

print "==> Hillshading (3 azimuths)"
gdaldem hillshade $dem $"($d)/_a.tif" -az -120 -igor -compute_edges ...$co o> /dev/null
gdaldem hillshade $dem $"($d)/_b.tif" -az  60  -igor -compute_edges ...$co o> /dev/null
gdaldem hillshade $dem $"($d)/_c.tif" -az -45  -igor -compute_edges ...$co o> /dev/null

let info = gdalinfo -json $dem | from json
let w_px = $info.size.0
let h_px = $info.size.1
for name in [a b c] {
    let raw = $"($d)/_($name)_raw.tif"
    mv $"($d)/_($name).tif" $raw
    (gdal_translate -q -srcwin $CROP $CROP ($w_px - 2 * $CROP) ($h_px - 2 * $CROP)
      ...$co $raw $"($d)/_($name).tif" o> /dev/null)
    rm $raw
}

# ── Render at each zoom ───────────────────────────────────────────────────────

let pi = (1 | math arctan) * 4

let rendered = (
    $ZOOMS | each {|z|
        let tr = ($pi * 2 * 6378137 / 256 / (2 ** $z) | into string)
        print $"==> z($z): tr=($tr) m/px"

        for name in [a b c] {
            (gdalwarp -t_srs EPSG:3857 -tr $tr $tr -tap -r cubic -dstnodata none -of GTiff
              ...$co -multi -wo NUM_THREADS=ALL_CPUS -wo INIT_DEST=0
              $"($d)/_($name).tif" $"($d)/($name)-w($z).tif" o> /dev/null)
        }

        let inputs = [-A $"($d)/a-w($z).tif" -B $"($d)/b-w($z).tif" -C $"($d)/c-w($z).tif"]
        let co_calc = [--co=COMPRESS=ZSTD --co=PREDICTOR=2 --co=TILED=YES --co=NUM_THREADS=ALL_CPUS]

        gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/R($z).tif" $"--calc=(band-calc "0x20" "0xFF" "0x00")" o> /dev/null
        gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/G($z).tif" $"--calc=(band-calc "0x30" "0xEE" "0x00")" o> /dev/null
        gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/B($z).tif" $"--calc=(band-calc "0x60" "0x00" "0x00")" o> /dev/null
        gdal_calc.py ...$inputs ...$co_calc $"--outfile=($d)/A($z).tif" $"--calc=(alpha-calc)" o> /dev/null

        let vrt = $"($d)/stack($z).vrt"
        gdalbuildvrt -separate $vrt $"($d)/R($z).tif" $"($d)/G($z).tif" $"($d)/B($z).tif" $"($d)/A($z).tif" o> /dev/null
        gdal_edit.py -colorinterp_1 red -colorinterp_2 green -colorinterp_3 blue $vrt o> /dev/null
        sed -i '/<NoDataValue>/d; /<NODATA>/d; s/ComplexSource/SimpleSource/g' $vrt

        let out = $"($OUT_DIR)/z($z).tif"
        (gdal_translate -q -of GTiff ...$co $vrt $out o> /dev/null)

        let ri = (gdalinfo -json $out | from json)
        {
            zoom: $z
            tr: ($tr | into float)
            size: $ri.size
            bytes: (ls -l $out | get 0.size | into int)
            file: $out
        }
    }
)

$rendered | select zoom tr size bytes | print

# ── Compare each zoom against the finest, on the finest zoom's grid ───────────

let ref = ($rendered | last)
print $"==> Reference: z($ref.zoom)"

let comparisons = (
    $rendered | where zoom != $ref.zoom | each {|r|
        # Upscale to the reference grid — what a client does when it overzooms.
        let up = $"($d)/up_($r.zoom).tif"
        (gdalwarp -q -t_srs EPSG:3857 -tr $ref.tr $ref.tr -tap -r cubic
          -of GTiff ...$co
          $r.file $up o> /dev/null)

        # Align both to the identical grid before differencing: -tap can leave a
        # one-pixel size difference, and gdal_calc refuses mismatched extents.
        let refi = (gdalinfo -json $ref.file | from json)
        let upi  = (gdalinfo -json $up | from json)
        let nx = ([$refi.size.0 $upi.size.0] | math min)
        let ny = ([$refi.size.1 $upi.size.1] | math min)

        let a = $"($d)/cmp_ref_($r.zoom).tif"
        let b = $"($d)/cmp_up_($r.zoom).tif"
        gdal_translate -q -srcwin 0 0 $nx $ny -b 1 ...$co $ref.file $a o> /dev/null
        gdal_translate -q -srcwin 0 0 $nx $ny -b 1 ...$co $up $b o> /dev/null

        let stats = (python3 $"($d)/diff.py" $a $b | from json)

        print $"    z($r.zoom) upscaled to z($ref.zoom): mean ($stats.mean)/255, p95 ($stats.p95), ($stats.pct_over_5)% off by >5"
        {zoom: $r.zoom, vs: $ref.zoom, ...$stats}
    }
)

# ── Fixed-extent PNG crops for the visual comparison ──────────────────────────
# All crops cover the SAME ground and are written at the SAME pixel size, so what
# differs between them is only the detail the zoom captured — which is the
# comparison a human actually wants to make.

print "==> Writing PNG crops"
let cx0 = ($xmin + $CROP_OFF)
let cy1 = ($ymax - $CROP_OFF)
let cx1 = ($cx0 + $CROP_M)
let cy0 = ($cy1 - $CROP_M)

# Crop bounds in EPSG:3857, so the same ground window can be cut from each render.
let corners = (
    python3 $"($d)/reproj.py" $cx0 $cy0 $cx1 $cy1
      | str trim | split row " " | each {|v| $v | into float}
)

let png_px = 900
for r in $rendered {
    let png = $"($OUT_DIR)/z($r.zoom).png"
    # CUBIC, NOT NEAREST — this line decides whether the comparison is honest.
    # All three crops are blown up to the same pixel size so they can be laid
    # side by side; with -r nearest the coarser zooms come out as hard blocks,
    # which reads as "much worse" for a reason that has nothing to do with how
    # much terrain detail they carry. That is not what a map client does when it
    # overzooms, and judging z16 against z17 from nearest-neighbour crops
    # overstates the case for the finer zoom badly enough to have caused a wrong
    # call once already. Cubic matches the client, and matches the -r cubic used
    # for the numeric comparison above, so the picture and the statistics agree.
    (gdal_translate -q -of PNG
      -projwin $corners.0 $corners.3 $corners.2 $corners.1
      -outsize $png_px $png_px -r cubic
      $r.file $png o> /dev/null)
    print $"    ($png | path basename)"
}

# ── Report ────────────────────────────────────────────────────────────────────

# Disk extrapolation: the window is WIN_M^2, so scale its byte count to England's
# land area. The renders here are ZSTD, production is lossy JXL at distance 3.0
# and roughly a third the size, so both figures are given.
let win_km2 = ($WIN_M * $WIN_M / 1000000.0)
let deg = $pi / 180
let cos_n = (($LAT_MAX * $deg) | math cos)     # north end: smallest ground pixel
let cos_s = (($LAT_MIN * $deg) | math cos)     # south end: largest ground pixel
let projected = (
    $rendered | each {|r|
        let gb = ($r.bytes / $win_km2 * $ENGLAND_KM2 / 1024 / 1024 / 1024)
        {
            zoom: $r.zoom
            mercator_m_per_px: ($r.tr | math round -p 2)
            ground_m_per_px: $"(($r.tr * $cos_n) | math round -p 2)-(($r.tr * $cos_s) | math round -p 2)"
            sample_bytes: $r.bytes
            england_gb_zstd: ($gb | math round -p 0)
            england_gb_jxl_est: ($gb / 3 | math round -p 0)
        }
    }
)
$projected | print

{
    tile: $TILE
    window_m: $WIN_M
    extent_25833: {xmin: $xmin, ymin: $ymin, xmax: $xmax, ymax: $ymax}
    renders: ($rendered | select zoom tr size bytes)
    comparisons: $comparisons
    projected_disk: $projected
} | to json | save -f $"($OUT_DIR)/report.json"

rm -rf $d
print ""
print $"==> Done. ($OUT_DIR)/report.json, plus z*.png for eyeballing."
print "    Set ZOOM in shading-en.nu to whatever the numbers and the crops justify."
