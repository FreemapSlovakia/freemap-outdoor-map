#!/usr/bin/env nu

# Generate contour lines for Bayern from the 2 m smoothed DEM tiles that
# shading-de-by.nu emitted along the way. Port of contours-be.nu.
#
# Pipeline: /mnt/osm/de-by/smooth2m/*.tif  (1250x1250 px windows, 2 m, EPSG:25832)
#             -> one state VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:25832)
#
# THE CONTOURS MUST COME FROM THE SMOOTHED DEM. Contouring raw 1 m LiDAR gives
#   unusable spaghetti — every furrow and forest-floor speckle becomes a closed
#   loop. That is why the shading run bothers to emit smooth2m/ rather than
#   letting this script downsample the source itself. If smooth2m is empty, run
#   shading-de-by.nu first; do NOT point this at the DGM1 tiles as a shortcut.
#   (Luxembourg measured the difference: 46103 features unsmoothed against 30995
#   smoothed over identical terrain — a third of the lines were noise.)
#
# The consolidation pass is NOT skipped. gdal_contour over a many-thousand-tile
#   VRT is pathologically slow — scattered reads, tiles reopened per scanline —
#   so the tiles are merged into one contiguous raster first.
#
# NO DATUM HAZARD. EPSG:25832 is ETRS89 / UTM 32N, so the path to WGS 84 is a
#   null transform — nothing to install, nothing for PROJ to get silently wrong.
#   Contrast England (OSGB36, needs OSTN15) and Flanders (BD72, needs the IGN
#   NTv2 grid). This GPKG is native 25832 regardless.
#
# EDGE TRIANGULATION AT THE STATE BORDER. Measured 2026-09-07 near 50.209 N,
#   10.723 E: surface roughness falls from 0.0273 m in the interior to 0.0142 m
#   in the outer 50 m of coverage — the Bavarian DGM1 is TIN-interpolated where
#   the LiDAR ends. Contours inherit that: expect slightly too-smooth, slightly
#   too-straight lines within ~300 m of the state boundary. A cutline at the
#   administrative border would remove it; not applied yet.
#
# ELEVATION RANGE. Bayern spans roughly 100 m (Lower Main valley) to 2962 m
#   (Zugspitze), so at a 10 m interval expect ~290 distinct levels — far more
#   than the flat countries, and a correspondingly large feature count.
#
# Output: <18TB>/de-by/bayern_contours.gpkg (layer `cont_de_by_dtm`, EPSG:25832).
#
# ── HANDOFF TO POSTGIS — THE THINGS THAT BIT ON EARLIER COUNTRIES ─────────────
#
# 1. LOAD IT. The splitter now creates the destination table in its final
#    serving shape (height_m smallint, wkb_geometry geometry(LineString,3857))
#    and builds the GiST index itself when the load finishes, so the old dance
#    — hand-CREATE a wide table, then DROP ogc_fid, DROP id, cast height to
#    smallint, rename the column, rename the table, CREATE INDEX — is gone.
#
#      DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_by/bayern_contours.gpkg \
#          --source-table cont_de_by_dtm --dest-table contours_de_by \
#          --source-epsg 25832 --split-max-points 1000 \
#          --simplify-tolerance 2 --commit-interval 1000 --drop-existing
#
#    THE SPLITTER NEEDS THE PASSWORD IN THE URL. It uses rust-postgres, which
#    — unlike libpq — reads neither $PGPASSWORD nor ~/.pgpass, and fails with
#    "password missing" if the URL has none. So interpolate $PGPASSWORD into
#    the URL as above: the secret stays out of this public repo, and passing it
#    as an environment variable rather than --database-url also keeps it out of
#    `ps`. (psql, pg_dump and GDAL's PG driver do read $PGPASSWORD, so they
#    need no password in their connection strings at all.)
#
#    NOTE: `--simplify-high-quality` and `--drop-existing` are boolean FLAGS —
#    passing either a value fails with "unexpected argument".
#
#    PASS --simplify-tolerance 2 EXPLICITLY. The default is 1.0, and leaving it
#    out silently loads a denser table than en/lu/be/hr, which all used 2 (the
#    first Bayern load did exactly that: 1066570 rows / 1130 MB at the default).
#    The tolerance is applied in the SOURCE CRS, i.e. metres here.
#
#    smallint holds Bayern's range comfortably (max 2962 m).
#    Index naming: en/hr/no/se/lu/sk/be all use contours_<cc>_wkb_geometry_geom_idx.
#
# 2. pg_restore ONTO fm5 NEEDS --no-tablespaces (the dump carries this box's
#    `osm_ext`, which does not exist there) AND --no-owner (no `martin` role):
#
#      pg_restore --dbname=freemap --no-owner --no-privileges --no-tablespaces \
#        --single-transaction /tmp/contours_de_by.dump
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-by.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR   = "/mnt/osm/de_by"
const SRC_DIR    = "/mnt/osm/de_by/smooth2m"     # 2 m tiles from shading-de-by.nu
const INTERVAL   = 10                            # contour interval, metres
const HEIGHT_COL = "height"
const NODATA     = "-9999"
const TABLE      = "cont_de_by_dtm"              # layer name inside the GPKG
const VRT        = "bayern_dem_2m.vrt"

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
let DEM_TIF = $"($DRIVE)/de_by/bayern_dem_2m.tif"      # consolidated DEM (EPSG:25832)
let GPKG    = $"($DRIVE)/de_by/bayern_contours.gpkg"   # splitter input (EPSG:25832)

print $"==> drive: ($DRIVE)"

# PROJ sanity — a missing PROJ database degrades every CRS to ENGCRS instead of
# failing loudly. Always run inside the geo env, never with its bin on PATH.
let _probe = (do { gdalsrsinfo -o proj4 "EPSG:25832" } | complete)
if $_probe.exit_code != 0 or ($_probe.stdout | str trim | is-empty) {
    error make {msg: "PROJ cannot resolve EPSG:25832 — run via: nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu contours-de-by.nu"}
}

cd $DATA_DIR

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — run shading-de-by.nu first \(it emits the smoothed 2 m tiles\)"}
}

# ── 1. State VRT straight from the 2 m tiles (no cropping needed) ─────────────

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing"
} else {
    print "==> Building state VRT from the 2 m tiles"
    let idx = "_idx_deby_cont"
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
# No reprojection and no resampling — the tiles are already EPSG:25832 at 2 m.

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

# ── 3. gdal_contour on the consolidated raster -> GPKG (EPSG:25832) ───────────

# ONE PASS AT -i 10 GETS OOM-KILLED ON A RASTER THIS BIG. Bayern consolidates
# to 180000 x 183750 (33 Gpx) and spans ~2860 m of relief, so a single
# `gdal_contour -i 10` has to hold ~290 levels of growing line work in memory at
# once. It was SIGKILLed by the OOM killer at 41 GB of 62 GB (2026-09-07).
# Belgium survived only because it is 15 Gpx with 83 levels.
#
# The fix is the one already used for Spain (es/contours_parallel.sh): run
# gdal_contour ten times at -i 100 with -off 0,10,...,90. Each pass carries a
# tenth of the levels and a fraction of the memory, and the union is identical
# to a single -i 10 run. Partials are merged with ogr2ogr -append.
#
# Resumable per offset: an existing partial that opens cleanly is skipped.
const OFF_INTERVAL = 100
const PARALLEL_OFF = 3                          # concurrent gdal_contour passes
const CACHEMAX_MB  = 2048                       # per process

if ($GPKG | path exists) {
    print $"==> ($GPKG) already exists — delete it to re-generate; skipping"
} else {
    let offsets = (0..(($OFF_INTERVAL / $INTERVAL) - 1) | each {|k| $k * $INTERVAL })
    print $"==> Generating contours in ($offsets | length) offset passes \(-i ($OFF_INTERVAL), -off ($offsets | str join ', ')\)"

    $offsets | par-each -t $PARALLEL_OFF {|off|
        let part = $"($DATA_DIR)/bayern_contours_off($off).gpkg"
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
    let missing = ($offsets | where {|off| not ($"($DATA_DIR)/bayern_contours_off($off).gpkg" | path exists) })
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
        let part = $"($DATA_DIR)/bayern_contours_off($it.item).gpkg"
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
    print $"    Next: splitter-rs \(--source-epsg 25832\); it now creates the table and index itself."
}
