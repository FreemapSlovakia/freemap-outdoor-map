#!/usr/bin/env nu

# Generate contour lines for all of Norway from the 2 m smoothed DEM tiles that
# shading-no.nu emitted along the way.
# Norway port of contours-hr.nu (Croatia 1 m) / contours-pl.nu (Poland 1 m).
#
# Pipeline: /mnt/osm/no/smooth2m/*.tif   (48101 tiles, 1500x1500 px, 2 m, EPSG:25833)
#             -> one national VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:25833)
#
# TWO DIFFERENCES FROM contours-hr.nu, both because shading-no.nu did the work up front:
#
#  * NO per-tile cropped VRTs. The Croatian script had to strip a 6 px overlap
#    from every smooth/ tile with its own gdal_translate -srcwin, 62351 of them.
#    shading-no.nu's dem2m step already wrote each window cropped to its exact
#    3 km extent (-projwin at the window bounds, collar excluded), so these tiles
#    tile the plane seamlessly with no overlap and no gaps. gdalbuildvrt is enough.
#
#  * NO 1 m -> 2 m downsampling. The tiles are already 2 m, produced with
#    nodata-aware `average` from the smoothed 1 m DEM while it was still in RAM.
#    That is the expensive half of Croatia's consolidation pass (8.5 h there;
#    it would have been ~45 h over Norway's area) already paid.
#
#    NOTE: the consolidation itself is NOT skipped — earlier notes of mine said
#    so and that was wrong. gdal_contour over a 48k-tile VRT is pathologically
#    slow (scattered reads, tiles reopened per scanline), exactly as the Croatian
#    header describes, so the tiles still have to be merged into one contiguous
#    raster first. What changed is that this pass now only copies 2 m data
#    instead of reading ~900 GB of 1 m data and resampling it.
#
# Consolidated DEM goes on the 18TB, not the NVMe: at Norway's bbox it is a few
# hundred GB and /mnt/osm has ~485 GB free with smooth2m already occupying 186 GB.
# gdal_contour reads it sequentially, so spinning rust costs little.
#
# nodata is clean: -9999 on every tile, presented unchanged by the VRT. Genuine
# 0.00 m coastal contours are preserved because out-of-coverage is -9999, never 0.
#
# Output: /mnt/osm/no/norway_contours.gpkg (layer `cont_no_dmr`, EPSG:25833).
# Handoff to the splitter (split <=1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/no/norway_contours.gpkg \
#       --source-table cont_no_dmr --dest-table cont_no_dmr_split \
#       --source-epsg 25833 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value fails with
# "unexpected argument". Omit it for fast Douglas-Peucker; pass it bare for Visvalingam.
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-no.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/no"
const SRC_DIR    = "/mnt/osm/no/smooth2m"                       # 2 m tiles from shading-no.nu
const INTERVAL   = 10                                           # contour interval, metres
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_no_dmr"                                # layer name inside the GPKG
const VRT        = "norway_dem_2m.vrt"
const DEM_TIF    = "/media/martin/18TB/no/norway_dem_2m.tif"    # consolidated DEM (EPSG:25833)
const GPKG       = "/mnt/osm/no/norway_contours.gpkg"           # splitter input (EPSG:25833)

cd $DATA_DIR

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run shading-no.nu first"}
}

# ── 1. National VRT straight from the 2 m tiles (no cropping needed) ──────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing"
} else {
    print "==> Building national VRT from the 2 m tiles"
    let idx = "_idx_no_cont"
    glob $"($SRC_DIR)/*.tif" | save -f $idx
    print $"  (open $idx | lines | length) tiles"
    gdalbuildvrt -vrtnodata $NODATA -input_file_list $idx $"($VRT).tmp" o> /dev/null
    rm $idx
    mv $"($VRT).tmp" $VRT
}

# ── 2. Consolidate the VRT into ONE contiguous raster ─────────────────────────
# No reprojection and no resampling — the tiles are already EPSG:25833 at 2 m.
# This exists purely so gdal_contour can read sequentially instead of reopening
# tiles per scanline.

if ($DEM_TIF | path exists) {
    print $"==> ($DEM_TIF) exists — reusing"
} else {
    print $"==> Consolidating DEM -> ($DEM_TIF) — one full pass"
    let tmp = $"($DEM_TIF).tmp"
    rm -f $tmp
    (gdal_translate
      --config GDAL_CACHEMAX 16384
      -of GTiff
      -a_nodata $NODATA
      -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES
      -co NUM_THREADS=ALL_CPUS -co BIGTIFF=YES
      $VRT $tmp)
    mv $tmp $DEM_TIF
}

# ── 3. gdal_contour on the consolidated raster -> GPKG (EPSG:25833) ───────────
# Single-threaded but reads sequentially. Resumable: delete the GPKG.

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
    print $"==> Done -> ($GPKG). Next: run splitter-rs \(--source-epsg 25833\); see header."
}
