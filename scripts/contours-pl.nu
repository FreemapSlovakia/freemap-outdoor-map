#!/usr/bin/env nu

# Generate contour lines for all of Poland from the smoothed DTM tiles.
# Poland port of contours.nu (which targets Spain / UTM zones 29-31).
#
# Pipeline: smooth/*.tif  → heal nodata=0 specks on affected tiles
#                         → per-tile cropped VRTs (drop 6 px overlap, no data copy)
#                         → one national VRT (EPSG:2180)
#                         → consolidate to ONE raster on SSD (gdal_translate, 2 m)
#                         → gdal_contour on that raster → GPKG (EPSG:2180)
#
# Why consolidate before contouring: gdal_contour over the 143k-tile VRT is
# pathologically slow — scattered reads, tiles reopened per scanline. Merging the
# VRT into a single contiguous raster first (one HDD pass) lets the contour read
# sequentially off the NVMe. Spain warped here because it spanned 3 UTM zones that
# had to be unified into one CRS; Poland is a single CRS (2180), so no reprojection
# is needed — a plain gdal_translate consolidates and downsamples 1 m → 2 m with
# nodata-aware `average` (verified: bilinear/cubic blend the 0 nodata, average does
# not). The splitter reprojects the vectors to 3857 later (exact — no raster
# resampling error).
#
# Output: /mnt/osm/poland_contours.gpkg (layer `cont_pl_dmr`, EPSG:2180).
# Handoff to the splitter (split ≤1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/poland_contours.gpkg \
#       --source-table cont_pl_dmr --dest-table cont_pl_dmr_split \
#       --source-epsg 2180 --split-max-points 1000 \
#       --simplify-tolerance 2 --simplify-high-quality false --commit-interval 1000
#
# nodata fix: GUGiK's WCS returns 0 both for out-of-coverage AND it collides with
# genuine 0.00 m coastal terrain (Żuławy). gdal_contour honours nodata, so those 0
# specks would notch the low contours. dem_zero_speck_tiles.txt lists the tiles
# with interior-0 specks (from the hillshade fix); we heal just those with
# gdal_fillnodata before contouring. Large out-of-coverage 0 regions (sea) stay
# nodata and are correctly excluded.
#
# Resumable: national VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Requires smooth/ populated. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-pl.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/run/media/martin/4TB/PL"
const CROP       = 6                             # px to drop from each tile edge
const INTERVAL   = 10                            # contour interval, metres
const HEIGHT_COL = "height"
const SPECK_LIST = "dem_zero_speck_tiles.txt"
const TABLE      = "cont_pl_dmr"                 # layer name inside the GPKG
const TR         = "2"                           # metres; downsample 1 m → 2 m for contours
const DEM_TIF    = "/mnt/osm/poland_dem_2m.tif"       # consolidated DEM on SSD (EPSG:2180)
const GPKG       = "/mnt/osm/poland_contours.gpkg"    # fast SSD, kept as splitter input (EPSG:2180)

cd $DATA_DIR

# ── 0. Heal nodata=0 specks on affected tiles → smooth_healed/<stem>.tif ───────

print "==> Healing nodata=0 specks on affected tiles"
mkdir smooth_healed
if ($SPECK_LIST | path exists) {
    (
      open $SPECK_LIST | lines | each {|s| $s | str trim } | where {|s| $s != "" }
        | where {|stem| not ($"smooth_healed/($stem).tif" | path exists) }
        | par-each -t 8 {|stem|
            let src = $"smooth/($stem).tif"
            if ($src | path exists) {
                let dst = $"smooth_healed/($stem).tif"
                let tmp = $"($dst).tmp"
                gdal_fillnodata.py -md 5 $src $tmp o> /dev/null e> /dev/null
                mv $tmp $dst
            }
          }
    )
} else {
    print $"  ($SPECK_LIST) not found — skipping heal \(low coastal contours may be notched\)"
}

# ── 1. Per-tile cropped VRT (drops 6 px/side, references healed tile if present) ─

print "==> Building per-tile cropped VRTs"
mkdir smooth_vrt
(
  glob smooth/*.tif
    | where {|f|
        let stem = $f | path basename | path parse | get stem
        not ($"smooth_vrt/($stem).vrt" | path exists)
      }
    | par-each -t 24 {|src|
        let stem   = $src | path basename | path parse | get stem
        let healed = $"smooth_healed/($stem).tif"
        let input  = if ($healed | path exists) { $healed } else { $src }
        let dst    = $"smooth_vrt/($stem).vrt"
        let info = gdalinfo -json $input | from json
        let w = $info.size.0
        let h = $info.size.1
        (gdal_translate -of VRT
          -srcwin $CROP $CROP ($w - 2 * $CROP) ($h - 2 * $CROP)
          $input $dst o> /dev/null)
      }
)

# ── 2. National VRT from the cropped per-tile VRTs (skip if present) ───────────

if ("poland_dem.vrt" | path exists) {
    print "==> National VRT exists — reusing"
} else {
    print "==> Building national VRT"
    let idx = "_idx_pl"
    glob smooth_vrt/*.vrt | save -f $idx
    let n = open $idx | lines | length
    print $"  ($n) tiles"
    gdalbuildvrt -vrtnodata 0 -input_file_list $idx poland_dem.vrt.tmp o> /dev/null
    rm $idx
    mv poland_dem.vrt.tmp poland_dem.vrt
}

# ── 2b. Consolidate the VRT into ONE raster on the SSD (skip if present) ───────
# No reprojection — Poland is one CRS (2180). gdal_translate merges the 143k-tile
# VRT into a single contiguous file and downsamples 1 m → 2 m with nodata-aware
# `average`, so the contour pass reads sequentially off the SSD.

if ($DEM_TIF | path exists) {
    print $"==> ($DEM_TIF) exists — reusing"
} else {
    print $"==> Consolidating DEM → ($DEM_TIF) \(EPSG:2180 @ ($TR) m\) — hours, one HDD pass"
    let tmp = $"($DEM_TIF).tmp"
    rm -f $tmp
    (gdal_translate
      --config GDAL_CACHEMAX 16384
      -of GTiff
      -tr $TR $TR -r average
      -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES
      -co NUM_THREADS=ALL_CPUS -co BIGTIFF=YES
      poland_dem.vrt $tmp o> /dev/null)
    mv $tmp $DEM_TIF
}

# ── 3. gdal_contour on the consolidated raster → GPKG (EPSG:2180) ──────────────
# Single-threaded but now reads sequentially off SSD. Resumable: delete GPKG.

if ($GPKG | path exists) {
    print $"==> ($GPKG) already exists — delete it to re-generate; skipping"
} else {
    print $"==> Generating contours → ($GPKG) — this will take hours"
    let tmp = $"($GPKG).tmp"
    rm -f $tmp
    (gdal_contour
      --config GDAL_CACHEMAX 16384
      -f GPKG
      -nln $TABLE
      -i $INTERVAL
      -a $HEIGHT_COL
      -snodata 0
      -lco SPATIAL_INDEX=NO
      $DEM_TIF $tmp)
    mv $tmp $GPKG
    print $"==> Done → ($GPKG). Next: run splitter-rs \(--source-epsg 2180\); see header."
}
