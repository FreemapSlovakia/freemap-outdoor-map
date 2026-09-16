#!/usr/bin/env nu

# Generate contour lines for Nordrhein-Westfalen from the 2 m smoothed DEM tiles
# that shading-de-nw.nu emitted along the way. Port of contours-de-by.nu.
#
# Pipeline: /mnt/osm/de-nw/smooth2m/*.tif  (1250x1250 px windows, 2 m, EPSG:25832)
#             -> one state VRT
#             -> consolidate to ONE contiguous raster on the 18TB
#             -> gdal_contour -> GPKG (EPSG:25832)
#
# THE CONTOURS MUST COME FROM THE SMOOTHED DEM. Contouring raw 1 m LiDAR gives
#   unusable spaghetti — every furrow and forest-floor speckle becomes a closed
#   loop. That is why the shading run bothers to emit smooth2m/ rather than
#   letting this script downsample the source itself. If smooth2m is empty, run
#   shading-de-nw.nu first; do NOT point this at the DGM1 tiles as a shortcut.
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
# EDGE TRIANGULATION AT THE STATE BORDER. Bayern's DGM1 was measured to be
#   TIN-interpolated in the outer ~50 m of coverage (roughness 0.0142 m against
#   0.0273 m inland), and the same is assumed here rather than re-measured.
#   Expect slightly too-smooth, too-straight lines within ~300 m of the NRW
#   boundary. A cutline at the administrative border would remove it; not
#   applied yet.
#
# ELEVATION RANGE. MEASURED -320 m to 840 m, i.e. ~116 levels at a 10 m
#   interval — a third of Bayern's ~290, but half again more than the ~85 the
#   natural terrain suggests.
#
#   THE NEGATIVE END IS REAL, NOT A NODATA ARTEFACT. NRW's lowest natural
#   ground is around 10 m on the Lower Rhine; the -320 m comes from the
#   open-cast lignite pits (Hambach, Garzweiler, Inden), which are the deepest
#   man-made holes in Europe and are surveyed like any other terrain. Do not
#   "fix" it with a floor. The top end is Langenberg in the Rothaargebirge at
#   843 m, hence a highest contour of 840.
#
#   smallint holds this with room to spare either way.
#
#   THAT MATTERS FOR MEMORY: the offset-pass machinery below exists because
#   Bayern's 290 levels over 33 Gpx got the single-pass run OOM-killed at 41 GB.
#   NRW is both flatter and half the size (~15 Gpx), i.e. comfortably inside
#   what Belgium did in one pass. The passes are kept anyway — they cost only
#   wall-clock, the union is identical, and guessing wrong costs an hour.
#
# Output: <18TB>/de-nw/nrw_contours.gpkg (layer `cont_de_nw_dtm`, EPSG:25832).
#
# ── HANDOFF TO POSTGIS ────────────────────────────────────────────────────────
#
# 1. LOAD IT. The splitter now creates the destination table in its final
#    serving shape (height_m smallint, wkb_geometry geometry(LineString,3857))
#    and builds the GiST index itself when the load finishes. The old dance —
#    hand-CREATE a wide table, then DROP ogc_fid, DROP id, cast height to
#    smallint, rename to height_m, rename the table, CREATE INDEX — is gone,
#    along with the chances to forget a step between countries.
#
#      DATABASE_URL="postgresql://martin:$PGPASSWORD@localhost/martin" \
#        /home/martin/fm/splitter/target/release/splitter-rs \
#          --source-gpkg <18TB>/de_nw/nrw_contours.gpkg \
#          --source-table cont_de_nw_dtm --dest-table contours_de_nw \
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
#    smallint holds NRW's range with room to spare (max 843 m). The index is
#    named contours_de_nw_wkb_geometry_geom_idx, matching en/hr/no/se/lu/sk/be.
#
# 2. pg_restore ONTO fm5 NEEDS --no-tablespaces (the dump carries this box's
#    `osm_ext`, which does not exist there) AND --no-owner (no `martin` role):
#
#      pg_restore --dbname=freemap --no-owner --no-privileges --no-tablespaces \
#        --single-transaction /tmp/contours_de_nw.dump
#
# Resumable: the VRT, consolidated raster and GPKG are each skipped if present
# (delete to force a rebuild). Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/contours-de-nw.nu

use lib/gdal.nu
use lib/contours.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const DATA_DIR = "/mnt/osm/de_nw"
const SRC_DIR  = "/mnt/osm/de_nw/smooth2m"
const EPSG     = "EPSG:25832"

gdal require-proj $EPSG "contours-de-nw.nu"

let DRIVE = (gdal find-drive)
print $"==> drive: ($DRIVE)"

contours run {
    code:         "denw"
    data_dir:     $DATA_DIR
    src_dir:      $SRC_DIR
    vrt:          $"($DATA_DIR)/nrw_dem_2m.vrt"
    dem_tif:      $"($DRIVE)/de_nw/nrw_dem_2m.tif"
    gpkg:         $"($DRIVE)/de_nw/nrw_contours.gpkg"
    table:        "cont_de_nw_dtm"                 # layer name inside the GPKG
    height_col:   "height"
    nodata:       "-9999"
    epsg:         $EPSG
    interval:     10
    off_interval: 100
    parallel_off: 3                                # concurrent gdal_contour passes
    cachemax_mb:  2048                             # per process
}
