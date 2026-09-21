# Render SEVERAL windows of a country at several zooms, through the real
# pipeline, so the ZOOM constant can be chosen from evidence instead of analogy.
#
# WHY THIS IS A LIBRARY AND WHY IT TAKES A LIST OF SITES.
#
#   The single-window samplers this replaces each hard-coded one `const TILE`
#   and verified nothing about it. Saxony's claimed to be the Bastei and was
#   actually a Dresden suburb 20 km away — measured 0.045 roughness against the
#   real sandstone's 0.300 — and nobody noticed for weeks, because its numbers
#   went from the log into a script header without anyone rendering the crops.
#   One window cannot represent a state, and a bulk pixel percentage dilutes
#   exactly the small isolated features a human judges by.
#
#   So: SITES ARE GIVEN AS lat/lon, NOT AS TILE NAMES. There is no filename to
#   mistype, and each site's coordinates AND its measured roughness are printed
#   in the log and written into the report, so a window that is not what it
#   claims to be is obvious on the first run.
#
# WHAT THE COMPARISON MEANS. The question is not "does a finer zoom hold more
#   numbers" — it trivially does. It is "does a finer zoom show the viewer
#   anything a cheaper one, stretched to the same screen size, does not". Each
#   coarser render is upscaled onto the finest zoom's grid, which is what a
#   client does when it overzooms, and differenced against the native render.
#
# TWO RULES THAT DECIDE WHETHER THE ANSWER IS HONEST:
#
#   * MEASURE ON SMOOTHED DATA. `--filter 11` strips most sub-11 m variation
#     before the hillshade exists, so differencing raw DEMs overstates the case
#     for a finer zoom by about 3x (Luxembourg: 8.5% raw against 2.6% smoothed).
#   * CROP WITH -r cubic, NEVER -r nearest. The crops are blown up to a common
#     pixel size for side-by-side viewing; nearest turns the coarser zooms into
#     hard blocks, which reads as "much worse" for a reason that has nothing to
#     do with terrain detail. That mistake caused a wrong call once already.
#
# ROUGHNESS is the std of the residual after a 3x3 mean — high-frequency energy
#   at the 1-3 m scale, which is what a hillshade renders. It is reported per
#   site so windows can be ranked, and because relief is the WRONG proxy:
#   Thüringen's Kyffhäuser has 220 m of relief against the Drachenschlucht's
#   160 m and is smoother per pixel; the Großer Beerberg, the state's highest
#   ground, is smoother still.
#
# cfg record:
#   code       country code, for scratch paths
#   src_dir    directory of source tiles (a VRT is built over ALL of them, so a
#              window near a tile edge still gets its collar)
#   out_dir    where z*.tif, z*.png and report.json are written
#   epsg       source CRS, e.g. "EPSG:25832"
#   nodata     source nodata, e.g. "-9999"
#   zooms      ascending list; the LAST is the reference, e.g. [15 16 17]
#   win_m      window edge in metres
#   collar     metres of context for the smoother
#   crop       pixels trimmed after hillshading
#   crop_off   offset of the PNG crop inside the window, metres
#   crop_m     PNG crop edge, metres
#   png_px     PNG crop output size, pixels
#   smooth     {filter, norm_diff, num_iter, max_diff}
#   fill_md    gdal_fillnodata -md for the sample only; 0 to skip
#   area_km2   land area, to extrapolate disk cost
#   lat_min/lat_max   latitude span, to convert Mercator px to ground metres
#   sites      list of {name, lat, lon, note}

const CO = [-co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS]

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

# Python helpers go to disk rather than `python3 -c`: nu's $"..." interpolation
# claims parentheses, so inlining python source into an interpolated string
# mangles every call. Plain single-quoted strings, arguments via argv.
def write-helpers [d: string]: nothing -> nothing {
    # COMPOSITE OVER WHITE BEFORE DIFFERENCING. The renders are RGBA and the
    # alpha carries the shading strength, so flat ground comes out almost fully
    # transparent — an East Frisian marsh window measured alpha 15/255, i.e.
    # 94% invisible. Differencing the colour bands alone then compares pixels
    # nobody sees, and reports noise as detail: that marsh scored 44.65% on the
    # raw bands against 3.39% composited, a 13x overstatement, and ranked as
    # losing more at z16 than the Harz. Hilly windows are overstated too, just
    # less (Harz 28.18% raw against 21.61% composited).
    #
    # The map paints these tiles over a light basemap, so compositing over white
    # is what the viewer actually gets. Compare the worst channel rather than the
    # mean so a shift in one colour is not diluted by two that held still.
    '
import json, sys
import numpy as np
from osgeo import gdal

def over_white(p):
    a = gdal.Open(p).ReadAsArray().astype("float32")
    if a.ndim == 2:                       # single band: nothing to composite
        return a[None, ...]
    if a.shape[0] < 4:                    # no alpha: take it as opaque
        return a[:3]
    al = a[3:4] / 255.0
    return a[:3] * al + 255.0 * (1.0 - al)

a = over_white(sys.argv[1])
b = over_white(sys.argv[2])
n = min(a.shape[1], b.shape[1]); m = min(a.shape[2], b.shape[2])
d = np.abs(a[:, :n, :m] - b[:, :n, :m]).max(axis=0)
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
t = Transformer.from_crs(sys.argv[1], "EPSG:3857", always_xy=True)
x0, y0, x1, y1 = (float(v) for v in sys.argv[2:6])
xs, ys = [], []
for x, y in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]:
    a, b = t.transform(x, y)
    xs.append(a); ys.append(b)
print(f"{min(xs)} {min(ys)} {max(xs)} {max(ys)}")
' | save -f $"($d)/reproj.py"

    # lat/lon -> source CRS, so sites are named by where they are rather than by
    # a filename nobody checks.
    '
import sys
from pyproj import Transformer
t = Transformer.from_crs("EPSG:4326", sys.argv[1], always_xy=True)
x, y = t.transform(float(sys.argv[3]), float(sys.argv[2]))
print(f"{x} {y}")
' | save -f $"($d)/fwd.py"

    # Roughness + relief of the window, so a window that is not what it claims
    # to be shows up immediately in the log.
    '
import json, sys
import numpy as np
from osgeo import gdal
from numpy.lib.stride_tricks import sliding_window_view
a = gdal.Open(sys.argv[1]).ReadAsArray().astype("float32")
a[a < -999] = np.nan
fill = np.nanmean(a)
af = np.nan_to_num(a, nan=fill)
sm = sliding_window_view(af, (3, 3)).mean(axis=(2, 3))
rough = float(np.nanstd(a[1:-1, 1:-1] - sm))
gy, gx = np.gradient(af)
slope = np.degrees(np.arctan(np.hypot(gx, gy)))
print(json.dumps({
    "roughness": round(rough, 4),
    "relief_m": round(float(np.nanmax(a) - np.nanmin(a)), 1),
    "slope_p50": round(float(np.percentile(slope, 50)), 1),
    "slope_p95": round(float(np.percentile(slope, 95)), 1),
    "void_pct": round(float(np.isnan(a).mean() * 100), 3),
}))
' | save -f $"($d)/terrain.py"
}

# ── One site: cut, smooth, hillshade, render every zoom, compare ──────────────

def render-site [site: record, cfg: record, d: string, vrt: string]: nothing -> record {
    let sd = $"($d)/($site.name)"
    rm -rf $sd
    mkdir $sd

    let xy = (python3 $"($d)/fwd.py" $cfg.epsg ($site.lat | into string) ($site.lon | into string)
                | str trim | split row " " | each {|v| $v | into float})
    let half = ($cfg.win_m / 2)
    let xmin = ($xy.0 - $half)
    let xmax = ($xy.0 + $half)
    let ymin = ($xy.1 - $half)
    let ymax = ($xy.1 + $half)

    print $"==> ($site.name): ($site.lat), ($site.lon)  ->  E ($xmin | math round) .. ($xmax | math round)  N ($ymin | math round) .. ($ymax | math round)"

    let win = $"($sd)/win.tif"
    (gdal_translate -q -of GTiff
      -projwin ($xmin - $cfg.collar) ($ymax + $cfg.collar) ($xmax + $cfg.collar) ($ymin - $cfg.collar)
      -co COMPRESS=DEFLATE -co PREDICTOR=1 -co TILED=YES
      $vrt $win o> /dev/null)

    let terrain = (python3 $"($d)/terrain.py" $win | from json)
    print $"    roughness ($terrain.roughness)  relief ($terrain.relief_m) m  slope p50 ($terrain.slope_p50)°  void ($terrain.void_pct)%"
    if $terrain.void_pct > 20 {
        print $"    !! ($site.name) is ($terrain.void_pct)% void — is this site inside the coverage?"
    }

    let smooth = $"($sd)/smooth.tif"
    (feature-preserving-smoothing --dem $win -o $smooth
      --filter $cfg.smooth.filter --norm_diff $cfg.smooth.norm_diff
      --num_iter $cfg.smooth.num_iter --max_diff $cfg.smooth.max_diff)

    let dem = if $cfg.fill_md > 0 {
        let filled = $"($sd)/dem.tif"
        gdal_fillnodata.py -md $cfg.fill_md $smooth $filled o> /dev/null err> /dev/null
        $filled
    } else {
        $smooth
    }

    gdaldem hillshade $dem $"($sd)/_a.tif" -az -120 -igor -compute_edges ...$CO o> /dev/null
    gdaldem hillshade $dem $"($sd)/_b.tif" -az  60  -igor -compute_edges ...$CO o> /dev/null
    gdaldem hillshade $dem $"($sd)/_c.tif" -az -45  -igor -compute_edges ...$CO o> /dev/null

    let info = (gdalinfo -json $dem | from json)
    for name in [a b c] {
        let raw = $"($sd)/_($name)_raw.tif"
        mv $"($sd)/_($name).tif" $raw
        (gdal_translate -q -srcwin $cfg.crop $cfg.crop ($info.size.0 - 2 * $cfg.crop) ($info.size.1 - 2 * $cfg.crop)
          ...$CO $raw $"($sd)/_($name).tif" o> /dev/null)
        rm $raw
    }

    let pi = (1 | math arctan) * 4

    let rendered = (
        $cfg.zooms | each {|z|
            let tr = ($pi * 2 * 6378137 / 256 / (2 ** $z) | into string)
            for name in [a b c] {
                (gdalwarp -t_srs EPSG:3857 -tr $tr $tr -tap -r cubic -dstnodata none -of GTiff
                  ...$CO -multi -wo NUM_THREADS=ALL_CPUS -wo INIT_DEST=0
                  $"($sd)/_($name).tif" $"($sd)/($name)-w($z).tif" o> /dev/null)
            }
            let inputs = [-A $"($sd)/a-w($z).tif" -B $"($sd)/b-w($z).tif" -C $"($sd)/c-w($z).tif"]
            let co_calc = [--co=COMPRESS=ZSTD --co=PREDICTOR=2 --co=TILED=YES --co=NUM_THREADS=ALL_CPUS]
            gdal_calc.py ...$inputs ...$co_calc $"--outfile=($sd)/R($z).tif" $"--calc=(band-calc "0x20" "0xFF" "0x00")" o> /dev/null
            gdal_calc.py ...$inputs ...$co_calc $"--outfile=($sd)/G($z).tif" $"--calc=(band-calc "0x30" "0xEE" "0x00")" o> /dev/null
            gdal_calc.py ...$inputs ...$co_calc $"--outfile=($sd)/B($z).tif" $"--calc=(band-calc "0x60" "0x00" "0x00")" o> /dev/null
            gdal_calc.py ...$inputs ...$co_calc $"--outfile=($sd)/A($z).tif" $"--calc=(alpha-calc)" o> /dev/null

            let stack = $"($sd)/stack($z).vrt"
            gdalbuildvrt -separate $stack $"($sd)/R($z).tif" $"($sd)/G($z).tif" $"($sd)/B($z).tif" $"($sd)/A($z).tif" o> /dev/null
            gdal_edit.py -colorinterp_1 red -colorinterp_2 green -colorinterp_3 blue $stack o> /dev/null
            sed -i '/<NoDataValue>/d; /<NODATA>/d; s/ComplexSource/SimpleSource/g' $stack

            let out = $"($cfg.out_dir)/($site.name)-z($z).tif"
            gdal_translate -q -of GTiff ...$CO $stack $out o> /dev/null
            let ri = (gdalinfo -json $out | from json)
            {zoom: $z, tr: ($tr | into float), size: $ri.size, bytes: (ls -l $out | get 0.size | into int), file: $out}
        }
    )

    let ref = ($rendered | last)
    let comparisons = (
        $rendered | where zoom != $ref.zoom | each {|r|
            let up = $"($sd)/up_($r.zoom).tif"
            (gdalwarp -q -t_srs EPSG:3857 -tr $ref.tr $ref.tr -tap -r cubic -of GTiff ...$CO
              $r.file $up o> /dev/null)
            let refi = (gdalinfo -json $ref.file | from json)
            let upi  = (gdalinfo -json $up | from json)
            let nx = ([$refi.size.0 $upi.size.0] | math min)
            let ny = ([$refi.size.1 $upi.size.1] | math min)
            let a = $"($sd)/cmp_ref_($r.zoom).tif"
            let b = $"($sd)/cmp_up_($r.zoom).tif"
            # All four bands, NOT -b 1: diff.py needs the alpha to composite.
            gdal_translate -q -srcwin 0 0 $nx $ny ...$CO $ref.file $a o> /dev/null
            gdal_translate -q -srcwin 0 0 $nx $ny ...$CO $up $b o> /dev/null
            let stats = (python3 $"($d)/diff.py" $a $b | from json)
            print $"    z($r.zoom) -> z($ref.zoom): mean ($stats.mean)/255, p95 ($stats.p95), ($stats.pct_over_5)% off by >5"
            {zoom: $r.zoom, vs: $ref.zoom, ...$stats}
        }
    )

    # Fixed-extent crops: same ground, same pixel size, so only captured detail differs.
    let cx0 = ($xmin + $cfg.crop_off)
    let cy1 = ($ymax - $cfg.crop_off)
    let corners = (
        python3 $"($d)/reproj.py" $cfg.epsg ($cx0 | into string) (($cy1 - $cfg.crop_m) | into string)
          (($cx0 + $cfg.crop_m) | into string) ($cy1 | into string)
          | str trim | split row " " | each {|v| $v | into float}
    )
    for r in $rendered {
        # CUBIC, NEVER NEAREST — see the header.
        (gdal_translate -q -of PNG -projwin $corners.0 $corners.3 $corners.2 $corners.1
          -outsize $cfg.png_px $cfg.png_px -r cubic
          $r.file $"($cfg.out_dir)/($site.name)-z($r.zoom).png" o> /dev/null)
    }

    rm -rf $sd

    {
        name: $site.name
        note: ($site | get -o note | default "")
        lat: $site.lat
        lon: $site.lon
        extent: {xmin: $xmin, ymin: $ymin, xmax: $xmax, ymax: $ymax}
        terrain: $terrain
        renders: ($rendered | select zoom tr size bytes)
        comparisons: $comparisons
    }
}

# ── Entry point ───────────────────────────────────────────────────────────────

export def run [cfg: record]: nothing -> nothing {
    if ($cfg.sites | is-empty) {
        error make {msg: "zoomsample needs at least one site"}
    }
    mkdir $cfg.out_dir
    let d = $"/dev/shm/($cfg.code)_zoomsample"
    rm -rf $d
    mkdir $d
    write-helpers $d

    # A VRT over ONLY the tiles the sites actually touch. Building one over the
    # whole state costs a gdalbuildvrt open of every tile: Saxony's 5k made that
    # unnoticeable, Bayern's 72k took longer than the renders themselves. The
    # windows are 2 km, so this is a handful of tiles per site.
    #
    # Tiles are matched by their FILENAME coordinates rather than by opening
    # them — every provider here names tiles <easting_km>_<northing_km> in some
    # arrangement, so the first two integers in the stem locate the tile. A stem
    # that yields no pair falls back to being included, which degrades to the old
    # behaviour rather than silently dropping data.
    print $"==> Selecting source tiles for ($cfg.sites | length) site\(s\)"
    let all = (ls $"($cfg.src_dir)/*.tif" | get name)

    let boxes = ($cfg.sites | each {|s|
        let xy = (python3 $"($d)/fwd.py" $cfg.epsg ($s.lat | into string) ($s.lon | into string)
                    | str trim | split row " " | each {|v| $v | into float})
        let pad = ($cfg.win_m / 2 + $cfg.collar + 2000)
        {x0: ($xy.0 - $pad), x1: ($xy.0 + $pad), y0: ($xy.1 - $pad), y1: ($xy.1 + $pad)}
    })

    # The NORTHING is found by range, not by position — position is wrong for
    # three of the four naming schemes in use:
    #
    #   Bayern      854_5405                        -> 854, 5405
    #   Sachsen     dgm1_33278_5590_2_sn            -> 33278, 5590   (trailing 2 = tile km)
    #   Thüringen   dgm1_32_561_5609_1_th_2020-2025 -> 561, 5609     (leading 32 = UTM zone)
    #   NRW         dgm1_32_531_5721_1_nw_2023      -> 531, 5721     (trailing 2023 = year)
    #
    # Taking the last two integers picks the tile-size flag or the year. Instead:
    # the first integer in 3000..8000 is the northing in km (no zone id, tile
    # size or year falls in that band anywhere in Europe), and the integer just
    # before it is the easting, with a UTM zone prefix stripped when present.
    let wanted = ($all | where {|f|
        let nums = ($f | path basename | path parse | get stem | split row "_"
                      | where {|p| ($p | find -r '^[0-9]+$' | is-not-empty) }
                      | each {|p| $p | into int })
        let ni = ($nums | enumerate | where {|r| $r.item >= 3000 and $r.item <= 8000 } | get -o 0.index)
        if $ni == null or $ni == 0 {
            true                                   # unrecognised: keep it
        } else {
            let e0 = ($nums | get ($ni - 1))
            let e = if $e0 > 1000 { $e0 mod 1000 } else { $e0 }
            let tx = $e * 1000
            let ty = ($nums | get $ni) * 1000
            # 2 km assumed, which over-includes for 1 km tiles — harmless.
            ($boxes | any {|b|
                $tx <= $b.x1 and ($tx + 2000) >= $b.x0 and $ty <= $b.y1 and ($ty + 2000) >= $b.y0
            })
        }
    })

    let picked = if ($wanted | is-empty) { $all } else { $wanted }
    print $"  ($picked | length) of ($all | length) tiles"
    let idx = $"($d)/srclist.txt"
    $picked | str join "\n" | save -f $idx
    let vrt = $"($d)/src.vrt"
    gdalbuildvrt -vrtnodata $cfg.nodata -input_file_list $idx $vrt o> /dev/null

    let results = ($cfg.sites | each {|s| render-site $s $cfg $d $vrt })

    # ── Summary, ranked by how rough the window actually is ───────────────────
    let ref_zoom = ($cfg.zooms | last)
    print ""
    print "==> Sites, roughest first"
    (
        $results
        | each {|r|
            let worst = ($r.comparisons | where zoom == ($ref_zoom - 1) | get -o 0.pct_over_5 | default null)
            {
                site: $r.name
                roughness: $r.terrain.roughness
                relief_m: $r.terrain.relief_m
                $"z(($ref_zoom - 1)) vs z($ref_zoom)": $worst
            }
        }
        | sort-by roughness --reverse
        | print
    )

    let pi = (1 | math arctan) * 4
    let deg = $pi / 180
    let cos_n = (($cfg.lat_max * $deg) | math cos)
    let cos_s = (($cfg.lat_min * $deg) | math cos)
    let win_km2 = ($cfg.win_m * $cfg.win_m / 1000000.0)

    # Disk is extrapolated from the ROUGHEST site, which compresses worst — the
    # honest upper bound. Saxony's extrapolation said 40 GB; the finished mosaic
    # was 10.9 GB, so expect the real figure to come in well under this.
    let roughest = ($results | sort-by {|r| $r.terrain.roughness} | last)
    let projected = (
        $roughest.renders | each {|r|
            let gb = ($r.bytes / $win_km2 * $cfg.area_km2 / 1024 / 1024 / 1024)
            {
                zoom: $r.zoom
                mercator_m_per_px: ($r.tr | math round -p 2)
                ground_m_per_px: $"(($r.tr * $cos_n) | math round -p 2)-(($r.tr * $cos_s) | math round -p 2)"
                gb_zstd_upper: ($gb | math round -p 0)
                gb_jxl_est_upper: ($gb / 3 | math round -p 0)
            }
        }
    )
    print ""
    print $"==> Disk, extrapolated from the roughest site \(($roughest.name)\)"
    $projected | print

    {
        code: $cfg.code
        epsg: $cfg.epsg
        window_m: $cfg.win_m
        zooms: $cfg.zooms
        sites: $results
        projected_disk: $projected
    } | to json | save -f $"($cfg.out_dir)/report.json"

    rm -rf $d
    print ""
    print $"==> Done. ($cfg.out_dir)/report.json, plus <site>-z*.png for eyeballing."
    print "    Judge the crops, not only the numbers: one window cannot represent a state,"
    print "    and a bulk pixel percentage dilutes small isolated features."
}
