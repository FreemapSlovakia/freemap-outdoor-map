#!/usr/bin/env python3
"""Build the Estonian 1 m tile queue that download-ee.nu fetches.

WHY A QUEUE AT ALL. The WCS coverage spans 375 x 260 km = 97,500 km2 against
45,339 km2 of land, so more than half of it is open Baltic. Asking for every
tile would waste over half the requests on sea that the service happily returns
as solid blocks of zeros.

HOW. The 25 m national model is cheap enough to pull whole — twelve 100 km
windows, which is what MAXSIZE=4096 allows at that resolution — and every
non-zero cell in it is land. That mask then selects which 4 km tiles of the 1 m
model are worth requesting.

THE MASK IS ITS OWN PROOF. Non-zero area came to 45,440 km2 against the
official 45,339: 0.2% out. It could not land that close if zero meant anything
other than sea, so this doubles as the check on the sentinel that
download-ee.nu's -srcnodata 0 depends on.

Writes scripts/ee_tiles.txt — one "x y" pair per line, the lower-left corner in
EPSG:3301. That file is generated, not source: it is rebuilt by running this,
the same way build-index-en.py rebuilds England's tile index.

Run via:
  nice ~/miniforge3/bin/conda run --no-capture-output -n geo python3 \
    ~/fm/freemap-outdoor-map/scripts/build-ee-mask.py
"""
import os
import subprocess
import sys
import urllib.request

import numpy as np
from osgeo import gdal

gdal.UseExceptions()

WCS = ('https://teenus.maaamet.ee/ows/wcs-dtm?service=WCS&version=2.0.1'
       '&request=GetCoverage&coverageId=dtm-25&format=image/tiff')
UA = {'User-Agent': 'Mozilla/5.0 (X11; Linux x86_64)'}

# The coverage extent, from DescribeCoverage. NOTE axisLabels="y x": the first
# value of each corner is the NORTHING. Getting this backwards returns
# "Image extent does not intersect with desired region", not a wrong tile.
X0, X1 = 365000, 740000
Y0, Y1 = 6375000, 6635000
STEP = 100000          # 100 km at 25 m = 4000 px, inside the server's MAXSIZE 4096
TILE = 4000            # m; the 1 m tile size download-ee.nu requests
HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, '.ee-mask')
QUEUE = os.path.join(HERE, 'ee_tiles.txt')
EE_LAND_KM2 = 45339.0


def fetch(xa, ya, xb, yb, path):
    url = f'{WCS}&subset=x({xa},{xb})&subset=y({ya},{yb})'
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=300) as r:
        data = r.read()
    # The service answers exceptions as XML with HTTP 200, so sniff the magic.
    if not (data.startswith(b'II') or data.startswith(b'MM')):
        return False
    open(path, 'wb').write(data)
    return True


def main():
    os.makedirs(WORK, exist_ok=True)
    parts = []
    for x in range(X0, X1, STEP):
        for y in range(Y0, Y1, STEP):
            xb, yb = min(x + STEP, X1), min(y + STEP, Y1)
            p = os.path.join(WORK, f't_{x}_{y}.tif')
            if os.path.exists(p) and os.path.getsize(p) > 10000:
                parts.append(p)
                continue
            if fetch(x, y, xb, yb, p):
                parts.append(p)
                print(f'  {x},{y} ok', flush=True)
            else:
                print(f'  {x},{y} FAILED', flush=True)
    if not parts:
        sys.exit('no windows fetched')

    vrt = os.path.join(WORK, 'ee25.vrt')
    subprocess.run(['gdalbuildvrt', '-q', '-overwrite', vrt] + parts, check=True)
    d = gdal.Open(vrt)
    a = d.GetRasterBand(1).ReadAsArray()
    gt = d.GetGeoTransform()
    d = None

    land = a != 0.0
    cell = (25 * 25) / 1e6
    km2 = land.sum() * cell
    print(f'\nland: {km2:,.0f} km2 against the official {EE_LAND_KM2:,.0f} '
          f'({abs(km2 - EE_LAND_KM2) / EE_LAND_KM2 * 100:.1f}% out)')
    if abs(km2 - EE_LAND_KM2) / EE_LAND_KM2 > 0.05:
        sys.exit('land area is more than 5% from the known figure — check the '
                 'mask before trusting the queue')

    ox, px, _, oy, _, py = gt
    H, W = land.shape
    out = []
    for ty in range(int(np.ceil((-py * H) / TILE))):
        for tx in range(int(np.ceil((px * W) / TILE))):
            x0 = ox + tx * TILE
            y1 = oy - ty * TILE
            c0, c1 = int((x0 - ox) / px), int((x0 + TILE - ox) / px)
            r0, r1 = int((oy - y1) / -py), int((oy - (y1 - TILE)) / -py)
            sub = land[max(r0, 0):min(r1, H), max(c0, 0):min(c1, W)]
            if sub.size and sub.any():
                out.append((int(x0), int(y1 - TILE)))

    with open(QUEUE, 'w') as fh:
        for x, y in out:
            fh.write(f'{x} {y}\n')
    print(f'{len(out)} land tiles of {TILE} m -> {QUEUE}')
    print(f'  {len(out) * (TILE / 1000) ** 2:,.0f} km2 of requests; the excess over '
          f'{EE_LAND_KM2:,.0f} is partial coastal tiles')


if __name__ == '__main__':
    main()
