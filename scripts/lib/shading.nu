# The shaded-relief pipeline: window grid -> per-window RGBA tile -> mosaic.
#
# One `run $cfg` call does the whole thing. The country script supplies the
# config and whatever source preparation is specific to it (downloading, or
# building the national VRT); everything below is shared mechanics.
#
# Config record — see any scripts/shading-*.nu for a worked example:
#
#   code       scratch prefix in $tmpdir; MUST be unique per country, or two
#              countries running at once share /dev/shm directories
#   src        raster the windows are cut from (a VRT or a single TIFF)
#   data_root  scratch root; smooth2m/ and failed/ live here
#   tiles_dir  per-window RGBA tiles (may sit outside data_root)
#   out_tif    final mosaic
#   nodata     nodata value stamped on the contour DEM
#   zoom       target web-mercator zoom — MEASURE it, do not assume
#   parallel   concurrent windows
#   tmpdir     ramdisk for per-window scratch
#   step       window size in METRES. Not uniform: 2500 m for a 1 m source,
#              1250 m for the Netherlands' 0.5 m one, so windows stay ~2500 px
#   collar     metres of context cut around each window for -compute_edges
#   crop       pixels trimmed from each hillshade edge, leaving collar - crop
#              as the warp margin
#   clamp      clamp -projwin to the source extent. Needed when the source is a
#              single raster whose extent is not step-aligned: gdal_translate
#              answers a partial overhang by silently returning a smaller
#              raster, which breaks the fixed-collar assumption in the crop
#   fill_md    gdal_fillnodata -md in PIXELS; 0 disables the step entirely
#   dem_tr     pixel size of the DEM handed to the contour pipeline, metres.
#              2 m for the 1 m and 0.5 m sources; a 5 m source has nothing to
#              gain from upsampling, so Italy keeps 5
#   smooth     {filter, norm_diff, num_iter, max_diff}
#   prefilter  null, or a {|src, dst| ...} closure run on the window BEFORE
#              smoothing. Exists for Italy: parts of its 5 m HRDTM are 10 m data
#              pixel-doubled into the grid, and feature-preserving smoothing
#              treats a staircase as a FEATURE and preserves it, so the repair
#              has to happen first. Skipped for flat windows, which have nothing
#              to repair
#   grid       {kind: "pinned", x0, y0, id_width}
#              or {kind: "tiles", origins: [{id, x0, y1}], nx, ny}

use gdal.nu *

# ── Window grid ───────────────────────────────────────────────────────────────

# The origin is PINNED, never derived from the extent. Deriving it was the
# Belgium trap: adding a region moved every window id and a resumable run
# treated thousands of finished tiles as pending.
def grid-pinned [cfg: record, ext: record]: nothing -> list<record> {
    let g = $cfg.grid
    if $ext.xmin < $g.x0 or $ext.ymin < $g.y0 {
        error make {msg: $"data extent \(($ext.xmin), ($ext.ymin)\) starts before the pinned grid origin \(($g.x0), ($g.y0)\) — lower x0/y0, but note that renames every tile"}
    }
    # Integer division on purpose: `/` yields a float and `0..0.0` is not
    # iterable in nu. `math floor`/`math ceil` give back ints.
    let i0 = ((($ext.xmin - $g.x0) / $cfg.step) | math floor)
    let i1 = (((($ext.xmax - $g.x0) / $cfg.step) | math ceil) - 1)
    let j0 = ((($ext.ymin - $g.y0) / $cfg.step) | math floor)
    let j1 = (((($ext.ymax - $g.y0) / $cfg.step) | math ceil) - 1)
    print $"  ($i1 - $i0 + 1) x ($j1 - $j0 + 1) grid"
    $i0..$i1 | each {|i|
        $j0..$j1 | each {|j|
            let xmin = ($g.x0 + ($i * $cfg.step | into float))
            let ymin = ($g.y0 + ($j * $cfg.step | into float))
            {
              id:   $"c(($i | fill -a r -c '0' -w $g.id_width))r(($j | fill -a r -c '0' -w $g.id_width))"
              xmin: $xmin
              xmax: ($xmin + ($cfg.step | into float))
              ymin: $ymin
              ymax: ($ymin + ($cfg.step | into float))
            }
        }
    } | flatten
}

# The other stable scheme: subdivide each SOURCE tile, so ids are anchored to
# the delivery grid and adding a tile cannot renumber its neighbours.
def grid-tiles [cfg: record]: nothing -> list<record> {
    let g = $cfg.grid
    $g.origins | each {|t|
        0..($g.nx - 1) | each {|i|
            0..($g.ny - 1) | each {|j|
                {
                  id:   $"($t.id)-($i)($j)"
                  xmin: ($t.x0 + ($i * $cfg.step | into float))
                  xmax: ($t.x0 + (($i + 1) * $cfg.step | into float))
                  ymin: ($t.y1 - (($j + 1) * $cfg.step | into float))
                  ymax: ($t.y1 - ($j * $cfg.step | into float))
                }
            }
        } | flatten
    } | flatten
}

# c* are the collared bounds actually cut and smoothed; d* the bare window the
# 2 m contour tile covers.
def window-bounds [w: record, cfg: record, ext: record]: nothing -> record {
    if $cfg.clamp {
        {
          cxmin: ([($w.xmin - $cfg.collar) $ext.xmin] | math max)
          cxmax: ([($w.xmax + $cfg.collar) $ext.xmax] | math min)
          cymin: ([($w.ymin - $cfg.collar) $ext.ymin] | math max)
          cymax: ([($w.ymax + $cfg.collar) $ext.ymax] | math min)
          dxmin: ([$w.xmin $ext.xmin] | math max)
          dxmax: ([$w.xmax $ext.xmax] | math min)
          dymin: ([$w.ymin $ext.ymin] | math max)
          dymax: ([$w.ymax $ext.ymax] | math min)
        }
    } else {
        {
          cxmin: ($w.xmin - $cfg.collar)
          cxmax: ($w.xmax + $cfg.collar)
          cymin: ($w.ymin - $cfg.collar)
          cymax: ($w.ymax + $cfg.collar)
          dxmin: $w.xmin
          dxmax: $w.xmax
          dymin: $w.ymin
          dymax: $w.ymax
        }
    }
}

export def window-grid [cfg: record]: nothing -> list<record> {
    let ext = (raster-extent $cfg.src)
    print $"  extent ($ext.xmin), ($ext.ymin) -> ($ext.xmax), ($ext.ymax)"
    let bare = match $cfg.grid.kind {
        "pinned" => (grid-pinned $cfg $ext)
        "tiles"  => (grid-tiles $cfg)
        _        => { error make {msg: $"unknown grid kind '($cfg.grid.kind)'"} }
    }
    # A clamped window can come out narrower than the crop takes off both sides,
    # which makes -srcwin ask for zero or negative width. gdal_translate fails,
    # the window lands in failed/, and `run` then refuses to merge on every
    # later run — a pipeline that can never finish without hand intervention.
    # Drop such slivers here instead: they carry at most a few metres of ground
    # that the neighbouring window's collar already covers.
    let px = (raster-res $cfg.src)
    let min_w = (2 * $cfg.crop + 1) * $px.x
    let min_h = (2 * $cfg.crop + 1) * $px.y
    (
      $bare
        | each {|w| $w | merge (window-bounds $w $cfg $ext) }
        | where {|w| ($w.cxmax - $w.cxmin) > $min_w and ($w.cymax - $w.cymin) > $min_h }
    )
}

# ── One window ────────────────────────────────────────────────────────────────

def render-window [w: record, cfg: record, tr: string]: nothing -> nothing {
    let out = $"($cfg.tiles_dir)/($w.id).tif"
    let d   = $"($cfg.tmpdir)/($cfg.code)_($w.id)"

    rm -rf $d
    mkdir $d

    # 1. Cut window + collar. PREDICTOR=1 — see CO_DEM in lib/gdal.nu.
    let win = $"($d)/win.tif"
    (gdal_translate -q -of GTiff
      -projwin $w.cxmin $w.cymax $w.cxmax $w.cymin
      ...$CO_DEM
      $cfg.src $win o> /dev/null)

    if not (has-data $win) {
        rm -rf $d
        touch $"($cfg.tiles_dir)/($w.id).empty"
        print $"  ($w.id): empty"
        return
    }

    # 2. Smooth. A zero-variance window panics the smoother and has nothing to
    #    smooth anyway — pass it through.
    let mm = (band-minmax $win)
    let smooth = $"($d)/smooth.tif"
    if $mm.min == $mm.max {
        cp $win $smooth
    } else {
        # Optional repair pass. It has to run BEFORE the smoother, which
        # preserves whatever it is given — see `prefilter` above.
        let dem_in = if $cfg.prefilter != null {
            let pre = $"($d)/pre.tif"
            do $cfg.prefilter $win $pre
            $pre
        } else {
            $win
        }
        (feature-preserving-smoothing --dem $dem_in -o $smooth
          --filter $cfg.smooth.filter --norm_diff $cfg.smooth.norm_diff
          --num_iter $cfg.smooth.num_iter --max_diff $cfg.smooth.max_diff)
    }

    # 3. Fill small voids — BEFORE the 2 m DEM is emitted, never after.
    #    Contouring the unfilled raster breaks a line at every building, tree
    #    clump and ditch while the hillshade beside it looks continuous,
    #    because the hillshade is the filled one. -md is in PIXELS, so the
    #    value scales with the source resolution.
    let dem = if $cfg.fill_md > 0 {
        let filled = $"($d)/dem.tif"
        gdal_fillnodata.py -md $cfg.fill_md $smooth $filled o> /dev/null err> /dev/null
        $filled
    } else {
        $smooth
    }

    # 4. Downsampled DEM for the contour pipeline — collar cropped, nodata-aware
    #    `average` (bilinear and cubic blend the nodata in; average does not).
    let dem2 = $"($cfg.data_root)/smooth2m/($w.id).tif"
    if not ($dem2 | path exists) {
        let tmp2 = $"($d)/dem2m.tif"
        (gdal_translate -q -of GTiff
          -projwin $w.dxmin $w.dymax $w.dxmax $w.dymin
          -tr $cfg.dem_tr $cfg.dem_tr -r average -a_nodata $cfg.nodata
          ...$CO $dem $tmp2 o> /dev/null)
        mv $tmp2 $dem2
    }

    # 5. Three Igor hillshades on the collared window so edges have neighbours.
    gdaldem hillshade $dem $"($d)/_a.tif" -az -120 -igor -compute_edges ...$CO o> /dev/null
    gdaldem hillshade $dem $"($d)/_b.tif" -az  60  -igor -compute_edges ...$CO o> /dev/null
    gdaldem hillshade $dem $"($d)/_c.tif" -az -45  -igor -compute_edges ...$CO o> /dev/null

    # 6. Crop the collar (minus the warp margin) off each hillshade.
    let size = (raster-size $dem)
    for name in [a b c] {
        let raw = $"($d)/_($name)_raw.tif"
        mv $"($d)/_($name).tif" $raw
        (gdal_translate -q -srcwin $cfg.crop $cfg.crop ($size.w - 2 * $cfg.crop) ($size.h - 2 * $cfg.crop)
          ...$CO $raw $"($d)/_($name).tif" o> /dev/null)
        rm $raw
    }

    # 7. Warp each to EPSG:3857 at zoom-level pixel size. -tap puts every window
    #    on the same global grid so the tiles mosaic without seams.
    for name in [a b c] {
        (gdalwarp -t_srs EPSG:3857 -tr $tr $tr -tap -r cubic -dstnodata none -of GTiff
          ...$CO_BIG -multi -wo NUM_THREADS=ALL_CPUS -wo INIT_DEST=0
          $"($d)/_($name).tif" $"($d)/($name)-warped.tif" o> /dev/null)
    }

    # 8. RGBA from the three warped hillshades.
    let inputs = [-A $"($d)/a-warped.tif" -B $"($d)/b-warped.tif" -C $"($d)/c-warped.tif"]

    #                       [a]    [b]    [c]
    let r_calc = band-calc "0x20" "0xFF" "0x00"
    let g_calc = band-calc "0x30" "0xEE" "0x00"
    let b_calc = band-calc "0x60" "0x00" "0x00"
    let a_calc = alpha-calc

    gdal_calc.py ...$inputs ...$CO_CALC $"--outfile=($d)/R.tif" $"--calc=($r_calc)" o> /dev/null
    gdal_calc.py ...$inputs ...$CO_CALC $"--outfile=($d)/G.tif" $"--calc=($g_calc)" o> /dev/null
    gdal_calc.py ...$inputs ...$CO_CALC $"--outfile=($d)/B.tif" $"--calc=($b_calc)" o> /dev/null
    gdal_calc.py ...$inputs ...$CO_CALC $"--outfile=($d)/A.tif" $"--calc=($a_calc)" o> /dev/null

    # 9. Stack RGBA with the alpha as an internal mask, then write the tile.
    let vrt_stack = $"($d)/stack.vrt"
    gdalbuildvrt -separate $vrt_stack $"($d)/R.tif" $"($d)/G.tif" $"($d)/B.tif" $"($d)/A.tif" o> /dev/null
    gdal_edit.py -colorinterp_1 red -colorinterp_2 green -colorinterp_3 blue $vrt_stack o> /dev/null
    sed -i '/<NoDataValue>/d; /<NODATA>/d; /<SrcRect/d; /<DstRect/d; s/ComplexSource/SimpleSource/g' $vrt_stack
    sed -i 's|</VRTDataset>|<MaskBand><VRTRasterBand dataType="Byte"><SimpleSource><SourceFilename relativeToVRT="1">a-warped.tif</SourceFilename><SourceBand>1</SourceBand></SimpleSource></VRTRasterBand></MaskBand></VRTDataset>|' $vrt_stack

    (gdal_translate --config GDAL_TIFF_INTERNAL_MASK YES -of GTiff
      ...$CO_BIG $vrt_stack $"($d)/final.tif" o> /dev/null)

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

def process-window [w: record, cfg: record, tr: string]: nothing -> nothing {
    try {
        render-window $w $cfg $tr
    } catch {|e|
        let msg = ($e | get -o msg | default "unknown")
        print $"  !! ($w.id): FAILED — ($msg)"
        touch $"($cfg.data_root)/failed/($w.id)"
        rm -rf $"($cfg.tmpdir)/($cfg.code)_($w.id)"
    }
}

# ── Mosaic ────────────────────────────────────────────────────────────────────

# Extent snapped outward so BOTH the origin and the dimensions are whole
# multiples of 2^levels pixels.
#
# The tiles are already on the global pixel grid (gdalwarp -tap, and the
# Mercator origin sits an exact 2^(zoom+7) pixels from projected zero), but
# their union is not. A pyramid built on a ragged extent lands off the tile
# grid, so GDAL cannot hand back a stored overview verbatim: every tile is
# resampled and low zooms sit up to half a pixel off. The padding is masked
# out, and costs ~1% of area — the Netherlands needed +1317 x +3113 px.
#
# levels tracks gdaladdo, which stops once a level fits in 256 px; tying the
# two together keeps the padding proportionate on small countries.
def aligned-extent [vrt: string, tr: float]: nothing -> list<string> {
    let info = (gdalinfo -json $vrt | from json)
    let ul = $info.cornerCoordinates.upperLeft
    let lr = $info.cornerCoordinates.lowerRight
    let maxdim = ([$info.size.0 $info.size.1] | math max)
    let levels = ([1 ((($maxdim / 256) | math log 2) | math ceil)] | math max)
    let q = $tr * (2 ** $levels)
    let xmin = ((($ul.0 / $q) | math floor) * $q)
    let ymax = ((($ul.1 / $q) | math ceil) * $q)
    let xmax = ((($lr.0 / $q) | math ceil) * $q)
    let ymin = ((($lr.1 / $q) | math floor) * $q)
    print $"  aligning to 2^($levels) px: ($info.size.0)x($info.size.1) -> (($xmax - $xmin) / $tr | math round)x(($ymax - $ymin) / $tr | math round)"
    [-te ($xmin | into string) ($ymin | into string) ($xmax | into string) ($ymax | into string)]
}

def merge-tiles [cfg: record]: nothing -> nothing {
    print "==> Merging tiles"
    let tiles = (glob $"($cfg.tiles_dir)/*.tif")
    print $"  ($tiles | length) tiles"
    build-vrt $tiles "shading.vrt" --index "shading_index"

    let te = (aligned-extent "shading.vrt" (zoom-tr $cfg.zoom | into float))
    build-vrt $tiles "shading.vrt" --extra $te --index "shading_index"

    sed -i 's|<ColorInterp>Alpha</ColorInterp>|<ColorInterp>Undefined</ColorInterp>|g' shading.vrt

    # JXL is lossy at distance 3.0 and needs a GDAL linked against a libtiff
    # with libjxl. PREDICTOR is deliberately absent — JXL does not use it.
    (gdal_translate --config GDAL_TIFF_INTERNAL_MASK YES --config GDAL_TIFF_INTERNAL_MASK_TO_8BIT YES
      -of GTiff -co COMPRESS=JXL -co JXL_LOSSLESS=NO -co JXL_DISTANCE=3.0
      -co TILED=YES -co BLOCKXSIZE=256 -co BLOCKYSIZE=256 -co BIGTIFF=YES -co NUM_THREADS=ALL_CPUS
      shading.vrt $"($cfg.out_tif).tmp")
    mv $"($cfg.out_tif).tmp" $cfg.out_tif
    rm shading.vrt
    gdal_edit.py -colorinterp_4 alpha $cfg.out_tif

    print "==> Building overviews"
    (gdaladdo --config GDAL_TIFF_INTERNAL_MASK YES --config GDAL_CACHEMAX 4096
      --config GDAL_NUM_THREADS ALL_CPUS --config COMPRESS_OVERVIEW JXL
      --config JXL_LOSSLESS_OVERVIEW NO --config JXL_DISTANCE_OVERVIEW 3.0
      -r average $cfg.out_tif)
}

# ── Pipeline ──────────────────────────────────────────────────────────────────

export def run [cfg: record]: nothing -> nothing {
    if not ($cfg.src | path exists) {
        error make {msg: $"($cfg.src) not found — has the source been downloaded and the VRT built?"}
    }

    mkdir $cfg.data_root
    mkdir $cfg.tiles_dir
    mkdir $"($cfg.data_root)/smooth2m"
    mkdir $"($cfg.data_root)/failed"
    mkdir ($cfg.out_tif | path dirname)
    cd $cfg.data_root

    let tr = (zoom-tr $cfg.zoom)
    print $"ZOOM=($cfg.zoom) TR=($tr)"

    print "==> Building window list"
    let windows = (window-grid $cfg)
    print $"  ($windows | length) windows"

    let pending = (
        $windows | where {|w|
            not ($"($cfg.tiles_dir)/($w.id).tif" | path exists) and not ($"($cfg.tiles_dir)/($w.id).empty" | path exists)
        }
    )
    print $"==> ($windows | length) windows, ($pending | length) pending"

    if ($pending | length) > 0 {
        $pending | par-each -t $cfg.parallel {|w| process-window $w $cfg $tr}
    }

    # Refuse to merge a partial state. Once out_tif exists every later run skips
    # straight past the merge, so a mosaic built while windows were still
    # missing would quietly become the final product.
    let unfinished = (
        $windows | where {|w|
            not ($"($cfg.tiles_dir)/($w.id).tif" | path exists) and not ($"($cfg.tiles_dir)/($w.id).empty" | path exists)
        }
    )
    let failed = (glob $"($cfg.data_root)/failed/*" | each {|f| $f | path basename })

    print "==> Checking every tile has 4 bands (gdalbuildvrt skips those that do not)"
    let malformed = (
        glob $"($cfg.tiles_dir)/*.tif"
          | par-each -t $cfg.parallel {|f|
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
            print $"    ($failed | length) window\(s\) in ($cfg.data_root)/failed/ — delete the marker\(s\) and re-run to retry:"
            $failed | first 10 | each {|f| print $"      ($f)" }
        }
        print "    Re-run this script; finished windows are skipped, so it resumes cheaply."
        exit 1
    }

    if ($cfg.out_tif | path exists) {
        print $"==> ($cfg.out_tif) exists — rename it \(don't delete\) to re-merge; skipping"
    } else {
        merge-tiles $cfg
    }
    print "==> Done"
}
