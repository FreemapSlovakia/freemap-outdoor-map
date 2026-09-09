#!/usr/bin/env nu

# Generate contour lines for all of Croatia from the smoothed 1 m DMR tiles.
# Croatia port of contours-pl.nu (Poland 1 m) / contours-it.nu (Italy 5 m).
#
# Pipeline: smooth/*.tif (from shading-hr.nu)
#             -> per-tile cropped VRTs (drop 6 px overlap, no data copy)
#             -> one national VRT (EPSG:3765)
#             -> consolidate to ONE raster on NVMe, downsampled 1 m -> 2 m
#             -> gdal_contour on that raster -> GPKG (EPSG:3765)
#
# Why consolidate before contouring: gdal_contour over a 62k-tile VRT is pathologically
# slow (scattered reads, tiles reopened per scanline). Merging into a single contiguous
# raster first lets the contour read sequentially off NVMe. Croatia is a single CRS
# (3765), so no reprojection — a plain gdal_translate consolidates AND downsamples
# 1 m -> 2 m with nodata-aware `average` (bilinear/cubic would blend the nodata into
# neighbours; average does not). 2 m is the right density for a 10 m interval and
# quarters the pixel count. The splitter reprojects the vectors to 3857 later.
#
# nodata is clean: this reads the SAME smooth/ tiles shading-hr.nu produced, whose
# source (hr.vrt) already unified the provider's mixed sentinels (-3.4e38 / -99 /
# -32767 / 0) to a single -9999. So NONE of Poland's zero-speck heal step is needed —
# that existed only because GUGiK's WCS overloaded 0 for out-of-coverage. Here -snodata
# -9999 cleanly excludes out-of-coverage and keeps genuine 0.00 m coastal contours.
#
# Output: /mnt/osm/hr/croatia_contours.gpkg (layer `cont_hr_dmr`, EPSG:3765).
# Handoff to the splitter (split <=1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/hr/croatia_contours.gpkg \
#       --source-table cont_hr_dmr --dest-table cont_hr_dmr_split \
#       --source-epsg 3765 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails with
# "unexpected argument". Omit it for fast Douglas-Peucker; pass it bare for Visvalingam.
#
# Resumable: the national VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Requires shading-hr.nu's smooth/ populated. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-hr.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/hr"
const CROP       = 6                             # px to drop from each tile edge (retile overlap is 12)
const INTERVAL   = 10                            # contour interval, metres
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_hr_dmr"                 # layer name inside the GPKG
const TR         = "2"                           # metres; downsample 1 m -> 2 m for contours
const VRT        = "croatia_dem.vrt"
const DEM_TIF    = "/mnt/osm/hr/croatia_dem_2m.tif"      # consolidated DEM (EPSG:3765)
const GPKG       = "/mnt/osm/hr/croatia_contours.gpkg"   # splitter input (EPSG:3765)

cd $DATA_DIR

if (not ("smooth" | path exists)) or ((glob smooth/*.tif | length) == 0) {
    error make {msg: "smooth/ is empty — run shading-hr.nu's smoothing stage first"}
}

# ── 1. Per-tile cropped VRTs (drop the 6 px/side overlap; references only, no copy) ─

print "==> Building per-tile cropped VRTs"
mkdir smooth_vrt
(
  glob smooth/*.tif
    | where {|f|
        let stem = $f | path basename | path parse | get stem
        not ($"smooth_vrt/($stem).vrt" | path exists)
      }
    | par-each -t 24 {|src|
        let stem = $src | path basename | path parse | get stem
        let dst  = $"smooth_vrt/($stem).vrt"
        let info = gdalinfo -json $src | from json
        let w = $info.size.0
        let h = $info.size.1
        (gdal_translate -of VRT
          -srcwin $CROP $CROP ($w - 2 * $CROP) ($h - 2 * $CROP)
          $src $dst o> /dev/null)
      }
)

# ── 2. National VRT from the cropped per-tile VRTs (skip if present) ───────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing"
} else {
    print "==> Building national VRT"
    let idx = "_idx_hr_cont"
    glob smooth_vrt/*.vrt | save -f $idx
    print $"  (open $idx | lines | length) tiles"
    gdalbuildvrt -vrtnodata $NODATA -input_file_list $idx $"($VRT).tmp" o> /dev/null
    rm $idx
    mv $"($VRT).tmp" $VRT
}

# ── 2b. Consolidate the VRT into ONE raster on NVMe, downsampled 1 m -> 2 m ─────
# No reprojection — Croatia is one CRS (3765). nodata-aware `average` resampling.

if ($DEM_TIF | path exists) {
    print $"==> ($DEM_TIF) exists — reusing"
} else {
    print $"==> Consolidating DEM -> ($DEM_TIF) \(EPSG:3765 @ ($TR) m\) — one full pass"
    let tmp = $"($DEM_TIF).tmp"
    rm -f $tmp
    (gdal_translate
      --config GDAL_CACHEMAX 16384
      -of GTiff
      -tr $TR $TR -r average
      -a_nodata $NODATA
      -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES
      -co NUM_THREADS=ALL_CPUS -co BIGTIFF=YES
      $VRT $tmp o> /dev/null)
    mv $tmp $DEM_TIF
}

# ── 3. gdal_contour on the consolidated raster -> GPKG (EPSG:3765) ─────────────
# Single-threaded but reads sequentially off NVMe. Resumable: delete the GPKG.

if ($GPKG | path exists) {
    print $"==> ($GPKG) already exists — delete it to re-generate; skipping"
} else {
    print $"==> Generating contours from ($DEM_TIF) -> ($GPKG) — this will take hours"
    let tmp = $"($GPKG).tmp"
    rm -f $tmp
    (gdal_contour
      --config GDAL_CACHEMAX 16384
      -f GPKG
      -nln $TABLE
      -i $INTERVAL
      -a $HEIGHT_COL
      -snodata $NODATA
      -lco SPATIAL_INDEX=NO
      $DEM_TIF $tmp)
    mv $tmp $GPKG
    print $"==> Done -> ($GPKG). Next: run splitter-rs \(--source-epsg 3765\); see header."
}
