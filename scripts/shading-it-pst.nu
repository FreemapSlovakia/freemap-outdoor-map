#!/usr/bin/env nu

# Shaded relief for the MASE PST lidar at z17 — the largest single contributor
# to Italy's cover, 131737 km² of the 320880 km² the eighteen sources hold
# between them.
#
# Source: /run/media/martin/18TB/it-pst/pst.tif — Piano Straordinario di
#   Telerilevamento, 1 m lidar flown 2008-15, already warped to EPSG:3857 on the
#   XYZ z17 grid (1.194329 m), Int32 in 2 mm steps, nodata -2147483648.
#   Read-only; it belongs to the Italy source build.
#
# LICENCE CC BY 4.0, Ministero dell'Ambiente e della Sicurezza Energetica. The
#   credit is in write-dtm-attribution.nu under `it_pst` and carries the
#   modification marker, because a hillshade is a derived work.
#
# PST OVERRIDES THE REGIONAL GRIDS WHEREVER IT REACHES, which is why it keeps
#   95.5% of its own area against Sardegna's 43% and Toscana's 35%: it sits
#   later in the stack than all of them. Only trento, emilia05 and fvg05 are
#   finer, so those three mask it.
#
# NOT italy.vrt — that mosaic is on the same z17 grid, but it has already
#   resolved the stack, so cutting from it would hand this dataset ground that
#   belongs to a different authority and a different credit. Each source is cut
#   from its own raster; the renderer resolves overlap from the masks.
#
# Int32 IN 2 mm STEPS, so `unscale` is set. gdaldem ignores the band's Scale tag
#   and would hillshade heights 500x too large; the smoother reads raw bytes and
#   would see the same.
#
# STEP IS 3000 m, which is 2512 px on a 1.194 m source — the ~2500 px window the
#   pipeline is tuned for. The step is in metres and does not transfer between
#   sources of different resolution: the 0.5 m scripts use 1500.
#
# THE GRID IS MOSTLY EMPTY. 523 x 575 windows cover the bounding box, and PST
#   holds about half of Italy in broken strips along rivers, coasts and
#   landslide areas, so most cuts find nothing. Empty windows cost about 25 ms
#   each and leave a .empty marker.
#
# THE GRID ORIGIN IS PINNED below the extent (626172, 4383205) on the step grid,
#   so a source that grows later does not rename every window and strand a
#   resumable run.
#
# Resumable at window granularity. Run via:
#   nice ~/miniforge3/bin/conda run --no-capture-output -n geo nu ~/fm/freemap-outdoor-map/scripts/shading-it-pst.nu

use lib/gdal.nu
use lib/shading.nu

const SRC       = "/run/media/martin/18TB/it-pst/pst.tif"
const DATA_ROOT = "/mnt/osm/it_pst"

let DRIVE = (gdal find-drive)

shading run {
    code:      "it_pst"
    src:       $SRC
    unscale:   true                              # Int32, 2 mm steps
    data_root: $DATA_ROOT
    tiles_dir: $"($DATA_ROOT)/tiles"
    out_tif:   $"($DRIVE)/it/pst-shading.tif"
    nodata:    "-9999"
    zoom:      17                                # the source's own grid
    parallel:  12
    tmpdir:    "/dev/shm"
    step:      3000                              # m; 2512 px at 1.194329 m
    collar:    12                                # 10 px of context for -compute_edges
    crop:      3
    clamp:     true                              # single raster, extent not step-aligned
    fill_md:   5                                 # px; ~6 m at 1.194 m
    dem_tr:    2                                 # m; what the contour run reads
    smooth:    {filter: 11, norm_diff: 16, num_iter: 6, max_diff: 6}
    prefilter: null

    grid:      {kind: "pinned", x0: 624000, y0: 4383000, id_width: 3}
}
