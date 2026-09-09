#!/usr/bin/env nu

# Generate contour lines for all of Italy from the national 5 m HRDTM.
# Italy port of contours-pl.nu (which targets Poland's 1 m GUGiK DTM).
#
# Two sources are possible, selected by USE_SMOOTH:
#
#   USE_SMOOTH = false → contour /mnt/osm/it/it.tif directly. One command, no
#     staging, no extra disk. it.tif is already a single contiguous file on NVMe,
#     so the reason Poland needed the VRT → consolidate dance (143k tiles scattered
#     on an HDD, gdal_contour reopening tiles per scanline) simply does not exist
#     here. Lines are wigglier — the source carries LiDAR speckle plus, in places,
#     corduroy striping and TIN facets from contour interpolation.
#
#   USE_SMOOTH = true → reuse shading-it.nu's smooth/ tiles: crop each tile's
#     overlap into a per-tile VRT (no data copy), merge into one national VRT, then
#     consolidate to a single raster before contouring. Costs an extra ~25-30 GB and
#     one full pass, and REQUIRES shading-it.nu to have finished its smoothing stage.
#     Buys visibly calmer lines. Smoothing moves the surface only ~0.4 m mean / 2 m
#     p99 — far below the 10 m interval — so this changes line shape, not accuracy.
#
# No downsampling: Poland went 1 m → 2 m for the contour pass, but 5 m is already
# the right density for a 10 m interval, so the source resolution is kept as-is.
#
# nodata is -9999 throughout. Poland's `-snodata 0` MUST NOT be carried over: it
# would treat genuine 0.00 m coastal terrain as nodata and notch every low contour.
# Poland needed it only because its WCS overloaded 0 for out-of-coverage.
#
# Output: /mnt/osm/it/italy_contours.gpkg (layer `cont_it_dtm`, EPSG:6875).
# Handoff to the splitter (split ≤1000 pts, simplify, stream into PostGIS):
#   DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#     /home/martin/fm/splitter/target/release/splitter-rs \
#       --source-gpkg /mnt/osm/it/italy_contours.gpkg \
#       --source-table cont_it_dtm --dest-table cont_it_dtm_split \
#       --source-epsg 6875 --split-max-points 1000 \
#       --simplify-tolerance 2 --commit-interval 1000
#
# NOTE: `--simplify-high-quality` is a boolean FLAG — passing it a value
# (`--simplify-high-quality false`, as contours-pl.nu's header does) fails with
# "unexpected argument 'false'". Omit it for the fast Douglas-Peucker default;
# pass it bare to opt into Visvalingam.
#
# Resumable: the national VRT, consolidated raster and GPKG are each skipped if
# present (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/contours-it.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/it"
const SRC        = "/mnt/osm/it/it.tif"
const USE_SMOOTH = true                          # see header; false → contour it.tif directly
const CROP       = 6                             # px to drop from each tile edge (retile overlap is 12)
const INTERVAL   = 10                            # contour interval, metres
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_it_dtm"                 # layer name inside the GPKG
const VRT        = "italy_dem.vrt"
const DEM_TIF    = "/mnt/osm/it/italy_dem_5m.tif"        # consolidated smoothed DEM (EPSG:6875)
const GPKG       = "/mnt/osm/it/italy_contours.gpkg"     # splitter input (EPSG:6875)

cd $DATA_DIR

# ── 1. Pick the raster to contour ─────────────────────────────────────────────

let dem = if not $USE_SMOOTH {
    print $"==> Contouring the raw DTM directly \(USE_SMOOTH = false\)"
    $SRC
} else {
    if (not ("smooth" | path exists)) or ((glob smooth/*.tif | length) == 0) {
        error make {msg: $"smooth/ is empty — run shading-it.nu's smoothing stage first, or set USE_SMOOTH = false"}
    }

    # 1a. Per-tile cropped VRTs (drop the 6 px/side overlap; references only, no copy)
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

    # 1b. National VRT from the cropped per-tile VRTs
    if ($VRT | path exists) {
        print $"==> ($VRT) exists — reusing"
    } else {
        print "==> Building national VRT"
        let idx = "_idx_it"
        glob smooth_vrt/*.vrt | save -f $idx
        print $"  (open $idx | lines | length) tiles"
        gdalbuildvrt -vrtnodata $NODATA -input_file_list $idx $"($VRT).tmp" o> /dev/null
        rm $idx
        mv $"($VRT).tmp" $VRT
    }

    # 1c. Consolidate the VRT into ONE raster so the contour pass reads sequentially
    if ($DEM_TIF | path exists) {
        print $"==> ($DEM_TIF) exists — reusing"
    } else {
        print $"==> Consolidating DEM → ($DEM_TIF) \(EPSG:6875 @ 5 m\) — one full pass"
        let tmp = $"($DEM_TIF).tmp"
        rm -f $tmp
        (gdal_translate
          --config GDAL_CACHEMAX 16384
          -of GTiff
          -a_nodata $NODATA
          -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES
          -co NUM_THREADS=ALL_CPUS -co BIGTIFF=YES
          $VRT $tmp o> /dev/null)
        mv $tmp $DEM_TIF
    }

    $DEM_TIF
}

# ── 2. gdal_contour → GPKG (EPSG:6875) ────────────────────────────────────────
# Single-threaded, reads sequentially off NVMe. Resumable: delete the GPKG.

if ($GPKG | path exists) {
    print $"==> ($GPKG) already exists — delete it to re-generate; skipping"
} else {
    print $"==> Generating contours from ($dem) → ($GPKG) — this will take hours"
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
      $dem $tmp)
    mv $tmp $GPKG
    print $"==> Done → ($GPKG). Next: run splitter-rs \(--source-epsg 6875\); see header."
}
