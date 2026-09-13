#!/usr/bin/env nu

# Contours for the Netherlands from the 2 m smoothed tiles shading-nl.nu emits.
# Port of contours-de-nw.nu. Output: <18TB>/nl/nl_contours.gpkg, EPSG:28992.

# Must come from the smoothed DEM: contouring raw lidar gives spaghetti, every
# furrow a closed loop. Luxembourg measured 46103 features unsmoothed against
# 30995 smoothed over identical terrain.

# The consolidation pass is not skipped — gdal_contour over a many-thousand-tile
# VRT is pathologically slow, reopening tiles per scanline.

# Interval is 5 m, not the 10 m used elsewhere. Outside the Vaalserberg corner
# the country lies between about -7 m and +50 m, so 10 m would give almost
# nothing across the polders. Negative heights are real; do not floor them.

# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
#      DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/nl/nl_contours.gpkg \
#          --source-table cont_nl_dtm --dest-table contours_nl \
#          --source-epsg 28992 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
# The splitter needs the password IN the URL: it uses rust-postgres, which reads
# neither $PGPASSWORD nor ~/.pgpass. Interpolating it as above keeps the secret
# out of this public repo and out of `ps`. Pass --simplify-tolerance explicitly;
# the default of 1.0 silently loads a denser table than the other countries.
#
# --simplify-high-quality and --drop-existing are boolean FLAGS.
#
# pg_restore onto fm5 needs --no-tablespaces (the dump carries this box's
# osm_ext) and --no-owner (no `martin` role there):
#
#      pg_restore --dbname=freemap --no-owner --no-privileges --no-tablespaces \
#        --single-transaction /tmp/contours_nl.dump

# Resumable: VRT, consolidated raster and GPKG are each skipped if present.
# Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-nl.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/nl"
const SRC_DIR    = "/mnt/osm/nl/smooth2m"        # 2 m tiles from shading-nl.nu
const INTERVAL   = 5                             # metres — see header
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_nl_dtm"                 # layer name inside the GPKG
const VRT        = "nl_dem_2m.vrt"

# ── Drive discovery ───────────────────────────────────────────────────────────
# udisks2 moves the removable 18TB between /media and /run/media, and the unused
# path survives as an empty root-owned directory on / — a stale hardcoded path
# silently fills the root filesystem instead of failing.
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
let DEM_TIF = $"($DRIVE)/nl/nl_dem_2m.tif"      # consolidated DEM (EPSG:28992)
let GPKG    = $"($DRIVE)/nl/nl_contours.gpkg"   # splitter input (EPSG:28992)

print $"==> drive: ($DRIVE)"

# PROJ sanity — a missing PROJ database degrades every CRS to ENGCRS instead of
# failing loudly. Always run inside the geo env, never with its bin on PATH.
let _probe = (do { gdalsrsinfo -o proj4 "EPSG:28992" } | complete)
if $_probe.exit_code != 0 or ($_probe.stdout | str trim | is-empty) {
    error make {msg: "PROJ cannot resolve EPSG:28992 — run via: nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu contours-nl.nu"}
}

cd $DATA_DIR

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run shading-nl.nu first \(it emits the smoothed 2 m tiles\)"}
}

# ── 1. State VRT straight from the 2 m tiles (no cropping needed) ─────────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing"
} else {
    print "==> Building national VRT from the 2 m tiles"
    let idx = "_idx_nl_cont"
    glob $"($SRC_DIR)/*.tif" | save -f $idx
    print $"  (open $idx | lines | length) tiles"
    gdalbuildvrt -vrtnodata $NODATA -input_file_list $idx $"($VRT).tmp" o> /dev/null

    # gdalbuildvrt silently SKIPS inputs that disagree with the first on CRS,
    # band count or colour interpretation — it dropped a whole region that way
    # during the Belgium build. Verify every offered tile landed.
    let n_offered = (open $idx | lines | length)
    let n_used = (
        open --raw $"($VRT).tmp"
          | parse -r '<SourceFilename[^>]*>([^<]+)</SourceFilename>'
          | get capture0 | uniq | length
    )
    if $n_used != $n_offered {
        rm -f $"($VRT).tmp"; rm $idx
        error make {msg: $"gdalbuildvrt kept only ($n_used) of ($n_offered) tiles — the rest were silently skipped. Run it by hand to see the warnings."}
    }
    print $"  verified: all ($n_offered) tiles present"
    rm $idx
    mv $"($VRT).tmp" $VRT
}

# ── 2. Consolidate the VRT into ONE contiguous raster ─────────────────────────
# No reprojection and no resampling — the tiles are already EPSG:28992 at 2 m.

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

# ── 3. gdal_contour on the consolidated raster -> GPKG (EPSG:28992) ───────────

# Offset passes exist because Bayern (33 Gpx, ~290 levels) was OOM-killed in a
# single gdal_contour run; each pass carries a fraction of the levels and the
# union is identical. Here OFF_INTERVAL == INTERVAL, i.e. one ordinary pass:
# ~10.4 Gpx and ~66 levels is smaller and flatter than Belgium, which managed
# one pass. Raise to 25 or 50 if a run is ever killed.
const OFF_INTERVAL = 5
const PARALLEL_OFF = 3                          # concurrent gdal_contour passes
const CACHEMAX_MB  = 2048                       # per process

if ($GPKG | path exists) {
    print $"==> ($GPKG) already exists — delete it to re-generate; skipping"
} else {
    let offsets = (0..(($OFF_INTERVAL / $INTERVAL) - 1) | each {|k| $k * $INTERVAL })
    print $"==> Generating contours in ($offsets | length) offset passes \(-i ($OFF_INTERVAL), -off ($offsets | str join ', ')\)"

    $offsets | par-each -t $PARALLEL_OFF {|off|
        let part = $"($DATA_DIR)/nl_contours_off($off).gpkg"
        if ($part | path exists) {
            print $"  skip offset ($off) — already done"
        } else {
            print $"  start offset ($off)"
            (nice -n 10 gdal_contour
              --config GDAL_CACHEMAX $CACHEMAX_MB
              -f GPKG
              -nln $TABLE
              -i $OFF_INTERVAL
              -off $off
              -a $HEIGHT_COL
              -snodata $NODATA
              -lco SPATIAL_INDEX=NO
              $DEM_TIF $"($part).tmp")
            mv $"($part).tmp" $part
            print $"  done  offset ($off)"
        }
    }

    # Every partial must exist before merging, or the country silently loses a
    # tenth of its contour levels — the same class of failure as merging a
    # partial shading mosaic.
    let missing = ($offsets | where {|off| not ($"($DATA_DIR)/nl_contours_off($off).gpkg" | path exists) })
    if ($missing | is-not-empty) {
        error make {msg: $"offsets ($missing | str join ', ') produced no output — re-run; finished offsets are skipped"}
    }

    # Merge with `enumerate` rather than `first`/`skip`. Both of those take
    # `oneof<table, binary, list<any>>` as pipeline input, and on this list they
    # blew up with "can't convert float to oneof<table, binary, list<any>>"
    # after all ten passes had already succeeded (2026-09-08) — an hour of
    # contouring saved only by the partials still being on disk. `enumerate`
    # turns the list into a table up front and the first/rest split becomes a
    # plain index test, so there is no pipeline-input coercion left to fail.
    print "==> Merging partials"
    let tmp = $"($GPKG).tmp"
    rm -f $tmp
    for it in ($offsets | enumerate) {
        let part = $"($DATA_DIR)/nl_contours_off($it.item).gpkg"
        if $it.index == 0 {
            print $"  base   offset ($it.item)"
            cp $part $tmp
        } else {
            print $"  append offset ($it.item)"
            ogr2ogr -update -append -nln $TABLE $tmp $part
        }
    }
    mv $tmp $GPKG
    print $"==> Done -> ($GPKG). Partials left in ($DATA_DIR) — delete once verified."
    print $"    Next: splitter-rs \(--source-epsg 28992\); it now creates the table and index itself."
}
