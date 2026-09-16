#!/usr/bin/env nu

# Generate shaded relief for Croatia from the national 1 m LiDAR DMR (DGU HR).
# Croatia port of shading.nu (Poland 1 m) / shading-it.nu (Italy 5 m).
#
# Source: /run/media/martin/2190983A5767510F/croatia-dtm/DGU_HR_LIDAR_G1_G2_DMR
#   — a flat tree of 62351 GeoTIFF tiles, each 1200 x 800 px, Float32, 1 m pixels,
#   EPSG:3765 (HTRS96 / Croatia TM). The tiles are the raw provider delivery and
#   carry two quirks that the Poland/Italy single-file sources did not:
#
#   QUIRK 1 — inconsistent nodata. The delivery mixes four nodata sentinels across
#   tiles: -3.4028235e+38 (~49%), -99 (~42%), -32767 (~8.5%) and 0 (~0.8%). All four
#   sit far below any real Croatian terrain, so each tile's own declared nodata is
#   correct for that tile. gdalbuildvrt honours per-source nodata (writes a ComplexSource
#   <NODATA> per tile) and lets us present ONE unified nodata (-9999) downstream —
#   see stage 0b. The ~0.8% of tiles that use 0 as nodata lose genuine 0.00 m pixels,
#   but those tiles are coastal/offshore where 0 = sea = out-of-coverage anyway.
#
#   QUIRK 2 — ~45% of tiles have NO embedded CRS (gdalinfo reports a null projection,
#   they ship only the .tfw geotransform). gdalbuildvrt normally SKIPS null-projection
#   tiles ("heterogeneous projection ... got (null)"), which would silently drop nearly
#   half the country. Every tile is genuinely EPSG:3765, so stage 0 builds the mosaic
#   with `-a_srs EPSG:3765 -allow_projection_difference`: -allow_projection_difference
#   stops the skip and -a_srs stamps the CRS onto the output. -a_srs ALONE is NOT enough
#   (it only labels output; the null tiles still mismatch the reference CRS and skip).
#   Safe because every tile IS EPSG:3765 and gdalbuildvrt never reprojects — the null
#   ones merely lack the label. Verified tile placement to the pixel (2026-07-30).
#
#   QUIRK 3 — 9 tiles declare NO nodata at all. gdalbuildvrt emits those as SimpleSource
#   (unmasked). Six are full-coverage mountain tiles (harmless), but three inland G2-W09
#   tiles carry UNMARKED 0-value LiDAR voids (10-35% exact-0 blobs amid 800-1000 m
#   relief). Stage 0 rewrites the SimpleSources to ComplexSource with <NODATA>0> so the
#   voids mask cleanly. NOTE for downstream users (e.g. elevation sampling on fm6): a
#   raw gdalbuildvrt would return 0 m over those three voids — apply the same fix.
#
# CONVERTED FROM gdal_retile TO THE SHARED WINDOW GRID (scripts/lib/shading.nu).
#   The old pipeline retiled the national VRT to disk (retiled/), smoothed every
#   tile to disk (smooth/), then hillshaded. That is ~2x the country written
#   twice; the window grid cuts each window from the VRT on demand, keeps its
#   intermediates in /dev/shm and deletes them on the way out. It also gets the
#   resumability, the .empty markers, the failed/ retry path and the band-count
#   guard that the retile scripts never had.
#
#   TILE IDS AND smooth2m/ ARE NEW. retiled/ and smooth/ are no longer produced,
#   and contours-hr.nu now reads smooth2m/. This needs a FULL RE-RENDER; the old
#   tiles/ names do not correspond to anything in the new grid. Delete tiles/,
#   retiled/ and smooth/ before the first run.
#
# Differences from the Poland script, and why:
#
#  * Source is a tile tree, so stage 0 normalises it into ONE national VRT (hr.vrt)
#    — assign CRS + unify nodata, both via gdalbuildvrt flags in a single command.
#    Non-destructive: the provider tiles on the external HDD are never modified and
#    nothing is written per tile; all state lives on NVMe.
#
#  * nodata is clean (each tile declares a real out-of-coverage sentinel), so NONE of
#    Poland's zero-speck / heal machinery applies — that existed only because GUGiK's
#    WCS overloaded 0 for both out-of-coverage AND genuine 0.00 m terrain. gdal_fillnodata
#    is still run per tile to close small interior voids (else transparent specks in the
#    relief); large out-of-coverage regions stay nodata -> transparent via the mask band.
#
#  * NO de-doubling. That was Italy-specific (its HRDTM mosaics 10 m data pixel-doubled
#    into the 5 m grid). Croatia is uniform 1 m LiDAR, so dedouble_dem.py is not used.
#
#  * PREDICTOR=1 on the window DEM is LOAD-BEARING, not a style choice. feature-preserving-
#    smoothing does I/O via the `wbgeotiff` crate, which does not parse the TIFF Predictor
#    tag (317) — fed PREDICTOR=2/3 float data it decodes byte-shuffled deltas as garbage
#    and emits ±Inf / f32::MAX rasters that render as static, WITHOUT erroring. The
#    shared library owns this now (CO_DEM in lib/gdal.nu).
#
#  * ZOOM=16, like Poland (1 m source). Croatia spans ~42.4-46.5 degN; z16 = 2.39
#    3857-m/px lands at ~1.6-1.8 ground m/px against a 1 m source (safely oversampled).
#    z15 would be ~3.3-3.5 ground m/px — undersampled, discarding real LiDAR detail.
#
#  * Smoothing is Poland's 11/16/6/6 (filter is a pixel count, so 11 px = 11 m on 1 m
#    data — matched to the source resolution, NOT Italy's softer 9/15/5/5 which was
#    scaled for 5 m pixels).
#
# THE GRID ORIGIN IS PINNED at (260000, 4690000), below the delivery extent
#   (263600, 4692400). Deriving it from the extent was the Belgium trap: adding a
#   region moves every window id and a resumable run treats finished tiles as
#   pending. Changing these renames every tile.
#
# Resumable at window granularity: hr.vrt and every tiles/<id>.tif are skipped if
# present, all-nodata windows leave a .empty marker, failures land in failed/ and
# are retried next run. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/shading-hr.nu

use lib/gdal.nu
use lib/shading.nu

# ── Configuration ─────────────────────────────────────────────────────────────

const SRC_DIR  = "/run/media/martin/2190983A5767510F/croatia-dtm/DGU_HR_LIDAR_G1_G2_DMR"
const DATA_DIR = "/mnt/osm/hr"                   # VRT, smooth2m/, tiles/ on NVMe
const EPSG     = "EPSG:3765"                     # HTRS96 / Croatia TM
const NODATA   = "-9999"                         # unified sentinel; sources carry four

let VRT = $"($DATA_DIR)/hr.vrt"

gdal require-proj $EPSG "shading-hr.nu"

if (not ($SRC_DIR | path exists)) or ((glob $"($SRC_DIR)/*.tif" | length) == 0) {
    error make {msg: $"($SRC_DIR) is empty — is the source drive mounted?"}
}

mkdir $DATA_DIR

# ── 0. National VRT: stamp the CRS, unify four nodata sentinels ───────────────
# Both provider quirks are fixed by flags, non-destructively — nothing is written
# per tile and the delivery on the external HDD is never modified:
#
#   * -a_srs EPSG:3765 + -allow_projection_difference: stamps the CRS onto the
#     output AND stops gdalbuildvrt skipping the ~45% of tiles that ship no CRS
#     (see header QUIRK 2 — -a_srs alone would still skip them, because it only
#     labels the output while the null tiles still mismatch the reference CRS).
#
#   * -vrtnodata -9999: presents one clean nodata downstream; each source keeps
#     its own real sentinel (-3.4e38 / -99 / -32767 / 0) as a per-source
#     <NODATA>, correctly masked.

if ($VRT | path exists) {
    print $"==> ($VRT) exists — reusing \(delete to force a rebuild\)"
} else {
    print $"==> Building national VRT ($VRT) — assign ($EPSG), unified nodata ($NODATA)"
    let tiles = (glob $"($SRC_DIR)/*.tif")
    print $"  ($tiles | length) tiles"
    let tmp = $"($VRT).tmp"
    let idx = $"($DATA_DIR)/_idx_hr"
    rm -f $tmp
    $tiles | str join "\n" | save -f $idx
    (gdalbuildvrt -a_srs $EPSG -allow_projection_difference -vrtnodata $NODATA
      -input_file_list $idx $tmp o> /dev/null)
    rm $idx
    gdal verify-vrt $tmp $tiles
    # QUIRK 3: the 9 tiles that declare no nodata at all come out as SimpleSource
    # (no per-source mask). Three carry UNMARKED 0-value LiDAR voids — 10-35%
    # exact-0 blobs amid 800-1000 m relief — which would otherwise render as
    # false flat patches in the hillshade and spurious 0 m contour rings.
    # Rewrite every SimpleSource -> ComplexSource with <NODATA>0 so those voids
    # mask like any other sentinel. gdalbuildvrt emits SimpleSource ONLY for
    # no-nodata sources (no tile's nodata equals the -9999 vrtnodata), so this
    # hits exactly those 9 and nothing else; it is harmless for the six that are
    # full-coverage mountain tiles, which contain no 0 px.
    sed -i 's|<SimpleSource>|<ComplexSource>|; s|</SimpleSource>|      <NODATA>0</NODATA>\n    </ComplexSource>|' $tmp
    mv $tmp $VRT
    print $"  verified: all ($tiles | length) tiles present in ($VRT)"
}

shading run {
    code:      "hr"
    src:       $VRT
    data_root: $DATA_DIR
    tiles_dir: $"($DATA_DIR)/tiles"
    out_tif:   $"($DATA_DIR)/shading.tif"
    nodata:    $NODATA
    zoom:      16                                # 1 m source — see header
    parallel:  24
    tmpdir:    "/dev/shm"
    step:      2500                              # m; = px at 1 m
    collar:    6
    crop:      3
    clamp:     false
    fill_md:   5                                 # px; closes small interior voids
    dem_tr:    2                                 # m; what contours-hr.nu reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 260000, y0: 4690000, id_width: 3}
}
