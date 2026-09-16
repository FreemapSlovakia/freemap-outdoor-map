# GDAL plumbing shared by the shading and contour pipelines.
#
# Everything here is mechanics. The measured, per-country findings — zoom,
# nodata, contour interval, datum notes — stay in the country scripts.

# Creation options, kept as constants so every pipeline writes the same TIFFs.
export const CO      = [-co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS]
export const CO_BIG  = [-co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS -co BIGTIFF=YES]
export const CO_CALC = [--co=COMPRESS=ZSTD --co=PREDICTOR=2 --co=TILED=YES --co=NUM_THREADS=ALL_CPUS --co=BIGTIFF=YES]

# PREDICTOR=1 is load-bearing on any DEM that feature-preserving-smoothing will
# read: it does I/O via wbgeotiff, which ignores the TIFF Predictor tag (317)
# and decodes PREDICTOR=2/3 float data as garbage (+/-Inf) WITHOUT erroring.
export const CO_DEM  = [-co COMPRESS=DEFLATE -co PREDICTOR=1 -co TILED=YES]

# udisks2 moves the removable 18TB between /media and /run/media, and the unused
# path survives as an empty root-owned directory on / — a stale hardcoded path
# silently fills the root filesystem instead of failing. Pick the real one.
export def find-drive []: nothing -> string {
    let found = (
        ["/run/media/martin/18TB" "/media/martin/18TB"]
          | where {|p| (do { mountpoint -q $p } | complete).exit_code == 0 }
    )
    if ($found | is-empty) {
        error make {msg: "the 18TB drive is not mounted at /media/martin/18TB or /run/media/martin/18TB. Check `lsblk -o NAME,LABEL,SIZE,MOUNTPOINT` (label 18TB)."}
    }
    $found | first
}

export def assert-mounted [p: string]: nothing -> nothing {
    if (do { mountpoint -q $p } | complete).exit_code != 0 {
        error make {msg: $"($p) is not a mountpoint — the drive is not mounted there. Check `lsblk -o NAME,LABEL,SIZE,MOUNTPOINT`."}
    }
}

# A missing PROJ database does not fail loudly: it degrades every CRS to
# ENGCRS["unnamed"] and the warp dies hours into the run with "Cannot find
# coordinate operations". Probe before any work is done.
export def require-proj [epsg: string, script: string]: nothing -> nothing {
    let probe = (do { gdalsrsinfo -o proj4 $epsg } | complete)
    if $probe.exit_code != 0 or ($probe.stdout | str trim | is-empty) {
        error make {msg: $"PROJ cannot resolve ($epsg) — run via: nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ($script)"}
    }
}

# A FALSE here writes a permanent .empty marker, so it must mean "gdalinfo ran
# and found no valid pixel", never "gdalinfo did not run".
export def has-data [file: string]: nothing -> bool {
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

# Returns {min, max}; either may be null when the band is entirely nodata.
export def band-minmax [file: string]: nothing -> record {
    let band = (gdalinfo -json -mm $file err> /dev/null | from json | get bands | first)
    {min: ($band | get -o computedMin), max: ($band | get -o computedMax)}
}

export def raster-extent [file: string]: nothing -> record {
    let gi = (gdalinfo -json $file | from json)
    let gt = $gi.geoTransform
    let xmin = $gt.0
    let ymax = $gt.3
    {
      xmin: $xmin
      ymax: $ymax
      xmax: ($xmin + ($gi.size.0 | into float) * $gt.1)
      ymin: ($ymax + ($gi.size.1 | into float) * $gt.5)
    }
}

# Pixel size in CRS units, always positive.
export def raster-res [file: string]: nothing -> record {
    let gt = (gdalinfo -json $file | from json | get geoTransform)
    {x: ($gt.1 | math abs), y: ($gt.5 | math abs)}
}

export def raster-size [file: string]: nothing -> record {
    let gi = (gdalinfo -json $file | from json)
    {w: $gi.size.0, h: $gi.size.1}
}

# The SourceFilenames gdalbuildvrt actually kept, by basename, deduplicated.
# A tile appears once per band plus once for the mask, hence the uniq.
export def vrt-sources [vrt: string]: nothing -> list<string> {
    open --raw $vrt
      | parse -r '<SourceFilename[^>]*>([^<]+)</SourceFilename>'
      | get capture0
      | each {|f| $f | path basename }
      | uniq
}

# gdalbuildvrt silently SKIPS inputs whose CRS, band count or colour
# interpretation disagrees with the first one, reporting it only in a warning
# swallowed by its progress bar. It dropped a whole region during the Belgium
# build. Nothing downstream can tell a dropped tile from a hole in the data, so
# every offered input is checked back out of the VRT.
export def verify-vrt [vrt: string, offered: list<string>]: nothing -> nothing {
    let want = ($offered | each {|f| $f | path basename } | uniq)
    let got  = (vrt-sources $vrt)
    let dropped = ($want | where {|f| $f not-in $got })
    if ($dropped | is-not-empty) {
        error make {msg: $"gdalbuildvrt dropped ($dropped | length) of ($want | length) inputs from ($vrt) — first few: ($dropped | first 10 | str join ', '). Run gdalbuildvrt by hand to see the warnings it swallowed."}
    }
}

# Build a VRT over `sources` and refuse to return one that lost an input.
# Writes through a .tmp so an interrupted run never leaves a half VRT that the
# next run would happily reuse.
export def build-vrt [
    sources: list<string>
    out: string
    --extra: list<string> = []      # e.g. [-vrtnodata -9999] or [-a_srs EPSG:3765]
    --index: string = "_idx_vrt"    # scratch file list, removed on success
]: nothing -> nothing {
    if ($sources | is-empty) {
        error make {msg: $"refusing to build ($out) from an empty source list"}
    }
    let tmp = $"($out).tmp"
    rm -f $tmp
    $sources | str join "\n" | save -f $index
    gdalbuildvrt ...$extra -input_file_list $index $tmp o> /dev/null
    try {
        verify-vrt $tmp $sources
    } catch {|e|
        rm -f $tmp
        rm -f $index
        error make {msg: ($e | get -o msg | default "gdalbuildvrt verification failed")}
    }
    rm -f $index
    mv $tmp $out
    print $"  verified: all ($sources | length) inputs present in ($out)"
}

# Weighted multi-directional hillshade blend for one RGB band. wa/wb/wc are the
# hex weights for the three azimuths.
export def band-calc [wa: string, wb: string, wc: string]: nothing -> string {
    let ea  = "0.8 * (255 - A)"
    let eb  = "0.7 * (255 - B)"
    let ec  = "1.0 * (255 - C)"
    let num = $"($ea) * ($wa) + ($eb) * ($wb) + ($ec) * ($wc)"
    let den = $"0.01 + ($ea) + ($eb) + ($ec)"
    "((" + $num + ") / (" + $den + ") - 128.0) + 128.0"
}

# Alpha: the inverse of "all three directions dark at once".
export def alpha-calc []: nothing -> string {
    let ea = "0.8 * (255 - A)"
    let eb = "0.7 * (255 - B)"
    let ec = "1.0 * (255 - C)"
    "255.0 - 255.0 * ((1.0 - " + $ea + " / 255.0) * (1.0 - " + $eb + " / 255.0) * (1.0 - " + $ec + " / 255.0))"
}

# Web-Mercator pixel size at a zoom level, as the string gdalwarp -tr wants.
export def zoom-tr [zoom: int]: nothing -> string {
    let pi = (1 | math arctan) * 4
    ($pi * 2 * 6378137 / 256 / (2 ** $zoom) | into string)
}
