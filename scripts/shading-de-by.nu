#!/usr/bin/env nu

# Generate shaded relief for Bayern (Germany) from the Bavarian DGM1.
# First of the German states; port of shading-be.nu.
#
# GERMANY IS DONE STATE BY STATE, one shading + contours product each, keyed
#   de-<state> on the ISO 3166-2:DE code. Bayern first: the data was already on
#   disk, it is the biggest outdoor draw (Alps, Bavarian Forest, Franconian
#   Jura), and at 70550 km2 it is 2.3x Belgium — a real test of the per-state
#   approach before the other fifteen.
#
# Source: /run/media/martin/2190983A5767510F/DGM1/Bayern — 71979 GeoTIFF tiles
#   of 1x1 km, 1 m, Float32, LZW, nodata -9999, EPSG:25832, with an all.vrt
#   already built over them. Downloaded via that directory's downloader.nu from
#   geodaten.bayern.de (OpenData, dl-de/by-2-0).
#
# EPSG:25832 IS ETRS89 / UTM 32N, so there is NO datum hazard: the path to
#   EPSG:3857 is a null transform, as it was for Wallonia's 3812. Contrast
#   Belgium's Flanders half (EPSG:31370 on BD72), which needed the IGN NTv2
#   grid. Nothing to install here.
#
# ZOOM=17, MEASURED 2026-09-04 on smoothed data through this exact pipeline.
#   Each coarser render resampled onto the z17 grid and differenced:
#
#     Berchtesgaden / Watzmann   z15 mean 10.81  p95 37  50.86% of px off by >5
#                                z16 mean  5.90  p95 21  31.90%
#     Bavarian Forest, Gr. Arber z16 mean  1.47  p95  5   4.50%
#     Danube plain, Ingolstadt   z16 mean  0.83  p95  3   2.38%
#     Altmuehltal karst          z16 mean  0.60  p95  2   1.26%
#
#   THE ALPS SETTLE IT AND IT IS NOT CLOSE. 31.90% against England's 3.2%,
#   Luxembourg's 2.6% and Wallonia's 4.2% — an order of magnitude more than any
#   terrain handled so far. Steep rock, couloirs and scree hold detail at 1 m
#   that survives --filter 11 and cannot be represented at z16's 1.6 ground
#   metres. A z17 pixel here is 0.81 ground metres against a 1 m source, i.e.
#   mildly oversampling, which is the right side to err on.
#
#   Note how strongly this varies WITHIN one state: the karst at 1.26% would
#   have been perfectly happy at z16. Do not assume the Alpine figure transfers
#   to the flat northern states — measure each, on SMOOTHED data. Differencing
#   unsmoothed renders overstates the case for a finer zoom by about 3x
#   (Luxembourg: 8.5% unsmoothed against 2.6% smoothed for the same test).
#
# NO gdal_fillnodata, MEASURED 2026-09-04. 60 random 1200x1200 windows, 31 of
#   them inland, 45 Mpx: 0.0000% nodata, NO voids of any size. The delivered
#   Bavarian DGM1 is gap-free within its footprint, as Flanders' DHMV II was.
#   (Luxembourg and Wallonia had voids, but every one was open water and none
#   was <= 25 px, so the step was dropped there too.) DGM1 is a different
#   product from a different authority, so this was measured rather than
#   assumed — do the same for each new state. To restore:
#     let dem = $"($d)/dem.tif"
#     gdal_fillnodata.py -md 5 $smooth $dem
#   and point step 5/6 at $dem instead of $smooth.
#
# EDGE ARTEFACTS AT THE STATE BORDER ARE ACCEPTED FOR NOW. Rendering a state in
#   isolation means -compute_edges extrapolates where a window has no neighbour
#   across the border, leaving a seam along every internal German boundary. The
#   fix is to include neighbouring states' tiles in the VRT as CONTEXT while
#   still only writing tiles inside this state's extent — cheap, but it needs
#   the neighbours downloaded first. Revisit once more states are in.
#
# PREDICTOR=1 on the window DEM is LOAD-BEARING — feature-preserving-smoothing
#   does I/O via `wbgeotiff`, which ignores the TIFF Predictor tag (317) and
#   decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
#
# THE GRID ORIGIN IS PINNED, NOT DERIVED FROM THE EXTENT. Deriving it was the
#   Belgium trap: adding a region moved the origin, every window id changed, and
#   a resumable run treated thousands of finished tiles as pending. Pinned below
#   the Bayern extent (498000, 5235717) on the STEP grid. Do NOT change these:
#   doing so renames every tile.
#
# ALWAYS RUN VIA `conda run -n geo`, never with the env's bin on PATH — that
#   leaves PROJ_DATA unset, degrades every CRS to ENGCRS["unnamed"], and the
#   warp fails hours in with "Cannot find coordinate operations".
#
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-de-by.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_VRT   = "/run/media/martin/2190983A5767510F/DGM1/Bayern/all.vrt"
const DATA_ROOT = "/mnt/osm/de_by"               # smooth2m/, tiles/ on NVMe
const TILES_DIR = "/mnt/osm/de_by/tiles"
const EPSG      = "EPSG:25832"                   # ETRS89 / UTM zone 32N
const NODATA    = "-9999"
const ZOOM      = 17                             # MEASURED — see header
const PARALLEL  = 24
const TMPDIR    = "/dev/shm"

const STEP      = 2500                           # window size, m (= px at 1 m)
const COLLAR    = 6
const CROP      = 3

const GRID_X0   = 495000
const GRID_Y0   = 5235000

const SM_FILTER    = 11
const SM_NORM_DIFF = 16
const SM_NUM_ITER  = 6
const SM_MAX_DIFF  = 6

# ── Drive discovery ───────────────────────────────────────────────────────────
# udisks2 moves the removable 18TB between /media and /run/media, and the unused
# path survives as an empty root-owned directory on / — a stale hardcoded path
# silently fills the root filesystem instead of failing. Pick the real one.
def find-drive []: nothing -> string {
    let found = (
        ["/run/media/martin/18TB" "/media/martin/18TB"]
          | where {|p| (do { mountpoint -q $p } | complete).exit_code == 0 }
    )
    if ($found | is-empty) {
        error make {msg: "the 18TB drive is not mounted at /media/martin/18TB or /run/media/martin/18TB. Check `lsblk -o NAME,LABEL,SIZE,MOUNTPOINT` (label 18TB)."}
    }
    $found | first
}

let DRIVE   = (find-drive)
let OUT_DIR = $"($DRIVE)/de_by"
let OUT_TIF = $"($OUT_DIR)/shading.tif"

print $"==> drive: ($DRIVE)"

if not ($SRC_VRT | path exists) {
    error make {msg: $"($SRC_VRT) not found — is the DGM1 drive mounted?"}
}

# PROJ sanity — a missing PROJ database does not fail loudly, it degrades every
# CRS to ENGCRS and breaks the warp long after the run has started.
let _probe = (do { gdalsrsinfo -o proj4 $EPSG } | complete)
if $_probe.exit_code != 0 or ($_probe.stdout | str trim | is-empty) {
    error make {msg: $"PROJ cannot resolve ($EPSG) — run via: nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu shading-de-by.nu"}
}

# ── Helpers ───────────────────────────────────────────────────────────────────

# A FALSE here writes a permanent .empty marker, so it must mean "gdalinfo ran
# and found no valid pixel", never "gdalinfo did not run".
def has-data [file: string]: nothing -> bool {
    let res = (do { gdalinfo -json -mm $file } | complete)
    if $res.exit_code != 0 {
        error make {msg: $"gdalinfo failed on ($file) — refusing to call it empty"}
    }
    let bands = ($res.stdout | from json | get -o bands | default [])
    if ($bands | is-empty) {
        error make {msg: $"gdalinfo reported no bands for ($file) — refusing to call it empty"}
    }
    (($bands | first) | get -o computedMin | is-not-empty)
}

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

def render-window [w: record, tr: string, data_dir: string]: nothing -> nothing {
    let out = $"($TILES_DIR)/($w.id).tif"
    let d   = $"($TMPDIR)/deby_($w.id)"

    rm -rf $d
    mkdir $d

    let co      = [-co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS]
    let co_big  = [...$co -co BIGTIFF=YES]
    let co_calc = [--co=COMPRESS=ZSTD --co=PREDICTOR=2 --co=TILED=YES --co=NUM_THREADS=ALL_CPUS --co=BIGTIFF=YES]

    # 1. Cut window + collar. PREDICTOR=1 — see header.
    let win = $"($d)/win.tif"
    (gdal_translate -q -of GTiff
      -projwin ($w.xmin - $COLLAR) ($w.ymax + $COLLAR) ($w.xmax + $COLLAR) ($w.ymin - $COLLAR)
      -co COMPRESS=DEFLATE -co PREDICTOR=1 -co TILED=YES
      $SRC_VRT $win o> /dev/null)

    if not (has-data $win) {
        rm -rf $d
        touch $"($TILES_DIR)/($w.id).empty"
        print $"  ($w.id): empty"
        return
    }

    # 2. Smooth. A zero-variance window panics the smoother and has nothing to
    #    smooth anyway — pass it through.
    let band = gdalinfo -json -mm $win err> /dev/null | from json | get bands | first
    let smooth = $"($d)/smooth.tif"
    if ($band.computedMin? == $band.computedMax?) {
        cp $win $smooth
    } else {
        (feature-preserving-smoothing --dem $win -o $smooth
          --filter $SM_FILTER --norm_diff $SM_NORM_DIFF
          --num_iter $SM_NUM_ITER --max_diff $SM_MAX_DIFF)
    }

    # 3. 2 m DEM for contours-de-by.nu — collar cropped, nodata-aware `average`.
    let dem2 = $"($data_dir)/smooth2m/($w.id).tif"
    if not ($dem2 | path exists) {
        let tmp2 = $"($d)/dem2m.tif"
        (gdal_translate -q -of GTiff
          -projwin $w.xmin $w.ymax $w.xmax $w.ymin
          -tr 2 2 -r average -a_nodata $NODATA
          ...$co $smooth $tmp2 o> /dev/null)
        mv $tmp2 $dem2
    }

    # 4. (no gdal_fillnodata — unmeasured for DGM1, see header)

    # 5. Three Igor hillshades on the collared window so edges have neighbours.
    gdaldem hillshade $smooth $"($d)/_a.tif" -az -120 -igor -compute_edges ...$co o> /dev/null
    gdaldem hillshade $smooth $"($d)/_b.tif" -az  60  -igor -compute_edges ...$co o> /dev/null
    gdaldem hillshade $smooth $"($d)/_c.tif" -az -45  -igor -compute_edges ...$co o> /dev/null

    # 6. Crop the collar (minus the warp margin) off each hillshade.
    let info = gdalinfo -json $smooth | from json
    let w_px = $info.size.0
    let h_px = $info.size.1
    for name in [a b c] {
        let raw = $"($d)/_($name)_raw.tif"
        mv $"($d)/_($name).tif" $raw
        (gdal_translate -q -srcwin $CROP $CROP ($w_px - 2 * $CROP) ($h_px - 2 * $CROP)
          ...$co $raw $"($d)/_($name).tif" o> /dev/null)
        rm $raw
    }

    # 7. Warp each to EPSG:3857 at zoom-level pixel size. -tap puts every window
    #    on the same global grid so the tiles mosaic without seams.
    for name in [a b c] {
        (gdalwarp -t_srs EPSG:3857 -tr $tr $tr -tap -r cubic -dstnodata none -of GTiff
          ...$co_big -multi -wo NUM_THREADS=ALL_CPUS -wo INIT_DEST=0
          $"($d)/_($name).tif" $"($d)/($name)-warped.tif" o> /dev/null)
    }

    # 8. RGBA from the three warped hillshades.
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

    # 9. Stack RGBA with the alpha as an internal mask, then write the tile.
    let vrt_stack = $"($d)/stack.vrt"
    gdalbuildvrt -separate $vrt_stack $"($d)/R.tif" $"($d)/G.tif" $"($d)/B.tif" $"($d)/A.tif" o> /dev/null
    gdal_edit.py -colorinterp_1 red -colorinterp_2 green -colorinterp_3 blue $vrt_stack o> /dev/null
    sed -i '/<NoDataValue>/d; /<NODATA>/d; /<SrcRect/d; /<DstRect/d; s/ComplexSource/SimpleSource/g' $vrt_stack
    sed -i 's|</VRTDataset>|<MaskBand><VRTRasterBand dataType="Byte"><SimpleSource><SourceFilename relativeToVRT="1">a-warped.tif</SourceFilename><SourceBand>1</SourceBand></SimpleSource></VRTRasterBand></MaskBand></VRTDataset>|' $vrt_stack

    (gdal_translate --config GDAL_TIFF_INTERNAL_MASK YES -of GTiff
      ...$co_big $vrt_stack $"($d)/final.tif" o> /dev/null)

    # gdal_calc.py can drop one of R/G/B/A without gdalbuildvrt -separate
    # complaining; gdalbuildvrt then SKIPS the tile at merge time behind a
    # warning swallowed by its progress bar, punching a silent hole in the
    # mosaic. England lost two tiles that way for real. Throw instead.
    let binfo = (gdalinfo -json $"($d)/final.tif" | from json | get bands)
    if ($binfo | length) != 4 {
        error make {msg: $"($w.id): produced ($binfo | length) bands, expected 4 — discarding"}
    }
    let ci = ($binfo | each {|b| $b | get -o colorInterpretation | default "" } | first 3)
    if $ci != [Red Green Blue] {
        error make {msg: $"($w.id): colour interpretation is ($ci), expected [Red Green Blue] — discarding"}
    }

    mv $"($d)/final.tif" $out
    rm -rf $d
    print $"  ($w.id): done"
}

def process-window [w: record, tr: string, data_dir: string]: nothing -> nothing {
    try {
        render-window $w $tr $data_dir
    } catch {|e|
        let msg = ($e | get -o msg | default "unknown")
        print $"  !! ($w.id): FAILED — ($msg)"
        touch $"($data_dir)/failed/($w.id)"
        rm -rf $"($TMPDIR)/deby_($w.id)"
    }
}

# ── Pipeline ──────────────────────────────────────────────────────────────────

mkdir $DATA_ROOT
mkdir $TILES_DIR
mkdir $"($DATA_ROOT)/smooth2m"
mkdir $"($DATA_ROOT)/failed"
mkdir $OUT_DIR
cd $DATA_ROOT

let pi = (1 | math arctan) * 4
let tr = ($pi * 2 * 6378137 / 256 / (2 ** $ZOOM) | into string)
print $"ZOOM=($ZOOM) TR=($tr)"

# ── 1. Window grid, absolute against the pinned origin ────────────────────────
print "==> Building window list"
let gi = (gdalinfo -json $SRC_VRT | from json)
let gt = $gi.geoTransform
let rx0 = $gt.0
let ry1 = $gt.3
let rx1 = ($rx0 + ($gi.size.0 | into float) * $gt.1)
let ry0 = ($ry1 + ($gi.size.1 | into float) * $gt.5)
print $"  extent ($rx0), ($ry0) -> ($rx1), ($ry1)"

if $rx0 < $GRID_X0 or $ry0 < $GRID_Y0 {
    error make {msg: $"data extent \(($rx0), ($ry0)\) starts before the pinned grid origin \(($GRID_X0), ($GRID_Y0)\) — lower GRID_X0/GRID_Y0, but note that renames every tile"}
}
let i0 = ((($rx0 - $GRID_X0) / $STEP) | math floor)
let i1 = (((($rx1 - $GRID_X0) / $STEP) | math ceil) - 1)
let j0 = ((($ry0 - $GRID_Y0) / $STEP) | math floor)
let j1 = (((($ry1 - $GRID_Y0) / $STEP) | math ceil) - 1)

let windows = (
    $i0..$i1 | each {|i|
        $j0..$j1 | each {|j|
            let xmin = ($GRID_X0 + ($i * $STEP | into float))
            let ymin = ($GRID_Y0 + ($j * $STEP | into float))
            {
              id:   $"c(($i | fill -a r -c '0' -w 3))r(($j | fill -a r -c '0' -w 3))"
              xmin: $xmin
              xmax: ($xmin + ($STEP | into float))
              ymin: $ymin
              ymax: ($ymin + ($STEP | into float))
            }
        }
    } | flatten
)
print $"  ($i1 - $i0 + 1) x ($j1 - $j0 + 1) grid -> ($windows | length) windows"

# ── 2. Process pending windows ────────────────────────────────────────────────
let pending = (
    $windows | where {|w|
        not ($"($TILES_DIR)/($w.id).tif" | path exists) and not ($"($TILES_DIR)/($w.id).empty" | path exists)
    }
)
print $"==> ($windows | length) windows, ($pending | length) pending"

if ($pending | length) > 0 {
    $pending | par-each -t $PARALLEL {|w| process-window $w $tr $DATA_ROOT}
}

# ── 3. Refuse to merge a partial state ────────────────────────────────────────
let unfinished = (
    $windows | where {|w|
        not ($"($TILES_DIR)/($w.id).tif" | path exists) and not ($"($TILES_DIR)/($w.id).empty" | path exists)
    }
)
let failed = (glob $"($DATA_ROOT)/failed/*" | each {|f| $f | path basename })

print "==> Checking every tile has 4 bands (gdalbuildvrt skips those that do not)"
let malformed = (
    glob $"($TILES_DIR)/*.tif"
      | par-each -t $PARALLEL {|f|
          let n = (do { gdalinfo -json $f } | complete)
          if $n.exit_code != 0 { {file: $f, bands: -1} } else { {file: $f, bands: ($n.stdout | from json | get bands | length)} }
        }
      | where bands != 4
)
if ($malformed | is-not-empty) {
    print $"==> ($malformed | length) malformed tile\(s\) — deleting so the next pass rebuilds them:"
    $malformed | first 20 | each {|m| print $"      ($m.file | path basename): ($m.bands) bands" }
    $malformed | each {|m| rm -f $m.file }
    print "    Re-run; refusing to merge a mosaic with holes."
    exit 1
}

if ($unfinished | is-not-empty) {
    print ""
    print $"==> NOT MERGING: ($unfinished | length) of ($windows | length) windows still have no output."
    if ($failed | is-not-empty) {
        print $"    ($failed | length) window\(s\) in ($DATA_ROOT)/failed/ — delete the marker\(s\) and re-run to retry:"
        $failed | first 10 | each {|f| print $"      ($f)" }
    }
    print "    Re-run this script; finished windows are skipped, so it resumes cheaply."
    exit 1
}

# ── 4. Merge and build overviews ──────────────────────────────────────────────
if ($OUT_TIF | path exists) {
    print $"==> ($OUT_TIF) exists — rename it \(don't delete\) to re-merge; skipping"
} else {
    print "==> Merging tiles"
    let idx = "shading_index"
    glob $"($TILES_DIR)/*.tif" | save -f $idx
    print $"  (open $idx | lines | length) tiles"
    gdalbuildvrt -input_file_list $idx shading.vrt

    # gdalbuildvrt silently SKIPS inputs whose band count, type or colour
    # interpretation disagrees with the first, reporting it only in a warning
    # swallowed by its progress bar. Verifying that every offered tile landed in
    # the VRT catches all such defects, including ones not yet met.
    let offered = (open $idx | lines | each {|f| $f | path basename})
    let used = (
        open --raw shading.vrt
          | parse -r '<SourceFilename[^>]*>([^<]+)</SourceFilename>'
          | get capture0 | each {|f| $f | path basename} | uniq
    )
    let dropped = ($offered | where {|f| $f not-in $used})
    if ($dropped | is-not-empty) {
        print ""
        print $"==> NOT MERGING: gdalbuildvrt dropped ($dropped | length) of ($offered | length) tiles."
        $dropped | first 20 | each {|f| print $"      ($f)" }
        print "    Delete those tiles and re-run; the windows will be regenerated."
        rm -f shading.vrt
        rm $idx
        exit 1
    }
    print $"  verified: all ($offered | length) tiles present in the VRT"

    rm $idx
    sed -i 's|<ColorInterp>Alpha</ColorInterp>|<ColorInterp>Undefined</ColorInterp>|g' shading.vrt

    (gdal_translate --config GDAL_TIFF_INTERNAL_MASK YES --config GDAL_TIFF_INTERNAL_MASK_TO_8BIT YES
      -of GTiff -co COMPRESS=JXL -co JXL_LOSSLESS=NO -co JXL_DISTANCE=3.0
      -co TILED=YES -co BLOCKXSIZE=256 -co BLOCKYSIZE=256 -co BIGTIFF=YES -co NUM_THREADS=ALL_CPUS
      shading.vrt $"($OUT_TIF).tmp")
    mv $"($OUT_TIF).tmp" $OUT_TIF
    rm shading.vrt
    gdal_edit.py -colorinterp_4 alpha $OUT_TIF

    print "==> Building overviews"
    (gdaladdo --config GDAL_TIFF_INTERNAL_MASK YES --config GDAL_CACHEMAX 4096
      --config GDAL_NUM_THREADS ALL_CPUS --config COMPRESS_OVERVIEW JXL
      --config JXL_LOSSLESS_OVERVIEW NO --config JXL_DISTANCE_OVERVIEW 3.0
      -r average $OUT_TIF)
}
print "==> Done"
