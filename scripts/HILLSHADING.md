# Hillshading and contours: what the pipeline does and what bites

Working notes for the DTM pipeline in `scripts/`. Every item here cost real time
to find; most were only visible after something shipped wrong. Read before
adding a country.

The per-country scripts are thin config; the pipeline is `lib/shading.nu`,
`lib/contours.nu` and `lib/gdal.nu`.

```
source DTM -> window grid -> smooth -> [fill] -> hillshade x3 -> RGBA tile
           -> mosaic (ALIGNED) -> overviews          -> shading.tif
           \-> 2 m DEM per window -> smooth2m/ -> VRT -> one raster
                                  -> gdal_contour -> GPKG -> splitter -> PostGIS
```

## Adding a country: the order that avoids rework

1. Download, build `all.vrt`, confirm CRS, nodata and pixel size from `gdalinfo`.
2. **Measure the zoom** with `sample-zoom-<cc>.nu` before rendering anything.
3. **Measure the voids** before choosing `fill_md`.
4. Write `shading-<cc>.nu`, run it.
5. Write `contours-<cc>.nu`, run it, load with `splitter-rs`.
6. Dump, restore on fm5, **verify as the serving role and with a real tile**.

---

## 1. Zoom selection

The question is not "does a finer zoom hold more numbers" — it trivially does.
It is "does a finer zoom show the viewer anything a cheaper one, stretched to
the same screen size, does not". `sample-zoom-<cc>.nu` renders a window at each
zoom, upscales the coarse one onto the fine grid (what a client does when it
overzooms), and reports the share of pixels off by more than 5/255.

Two rules, both learned the hard way:

- **Measure on SMOOTHED data.** Differencing unsmoothed renders overstates the
  case for the finer zoom by about 3x (Luxembourg: 8.5% unsmoothed vs 2.6%
  smoothed, same test). `feature-preserving-smoothing --filter 11` strips most
  sub-11 m variation before the hillshade is computed, so a zoom finer than the
  smoothed detail is resampling something already discarded.
- **Pick the roughest window in the country, on purpose.** Whatever conclusion
  holds on the most detailed terrain holds everywhere flatter. Roughness at the
  pixel scale is the thing, not relief: Saxony's sample used the Elbe sandstone
  (vertical walls, metre-scale structure), *not* the higher but glacially
  rounded Erzgebirge.

Two ways the roughness scan lies, both of which have picked the wrong window:

- **Roughness finds man-made steps before it finds mountains.** Quarry benches,
  spoil heaps, lignite pits and terraced suburbs all beat real terrain, because
  a bench edge is a true discontinuity. Sachsen-Anhalt's top seven included a
  quarry, a spoil heap and an active open-cast mine; Niedersachsen's single
  roughest window in the whole state was an open pit, and Saxony's "Bastei"
  sample was a Dresden suburb. **Render every candidate and look at it** before
  it reaches a sample list — the number alone cannot tell terrain from earthworks.
- **Voids must be excluded from the metric, not filled.** Substituting the
  window mean for nodata puts a cliff at every void edge, and the high-pass
  residual reads that cliff as terrain. Flat farmland on the Brandenburg border
  scored 0.159 that way, above most of the Harz. Score only 3x3 neighbourhoods
  that are entirely valid. The roughness figures in `sample-zoom-de-ni.nu`
  predate this and are inflated wherever a window met a border or a lake; that
  zoom decision rested on the rendered comparison, so it stands, but the
  roughness column does not.

Also beware: the PNG crops the sampler writes for eyeballing must be resampled
with `-r cubic`. `-r nearest` makes every zoom look equally blocky and has
actively misled a comparison before. `sample-zoom-en.nu` had this defect.

Measured so far: Bayern 31.90% at z16-vs-17 (Alps), Saxony 31.42% (sandstone),
England/Luxembourg/Wallonia in the 1–3% "subtle" band.

## 2. Void filling — measure, and split border from interior

`fill_md` is `gdal_fillnodata -md` in **pixels**, so its ground reach scales with
source resolution (10 px = 5 m at 0.5 m, 10 m at 1 m).

**The only voids worth filling are interior ones.** Nodata at the edge of
coverage is not damage — filling it invents terrain outside the country and
smears it across the seam with the neighbour. Flood-fill nodata from the window
border and see what is left:

- NRW looked alarming at 0.8240% raw nodata, but all of it was the state
  boundary. Border flood-fill consumed 510238 of 510238 px: 0.0000% interior.
  `fill_md: 0`.
- Saxony: 141 windows lying wholly inside the state, every one exactly 0.0000%.
  `fill_md: 0`. German DGM1 has buildings and vegetation already interpolated
  into the ground surface.
- AHN5 (Netherlands) is the opposite: a strict bare-earth product keeping only
  `maaiveld`-classified returns, so vegetation leaves **thin slivers** (1 px
  median, 98% under 6 px) and buildings/water leave **wide holes**.

Size tells you what a void is. Sliver runs of a few metres are canopy and should
be closed. Runs with a median of 138 m (Saxony) are lakes and open-cast mines and
must stay. A size threshold alone is not a safe rule — genuine interior holes
(the IJsselmeer) are kilometres across — which is why the border/interior split
is by connectivity, not by area.

Filling voids under buildings and water is harmless for rendering, because both
are drawn as their own layers on top of the shading.

## 3. Mercator alignment — the expensive one

**A mosaic whose extent is not a whole multiple of `2^levels` pixels produces an
overview pyramid that does not land on the Web Mercator tile grid.** GDAL then
cannot return a stored overview verbatim: every tile is resampled, and low zooms
sit up to half a pixel off.

The windows are already on the global pixel grid (`gdalwarp -tap`, and the
Mercator origin is an exact `2^(zoom+7)` pixels from projected zero), but their
*union* is not. `lib/shading.nu` now snaps the mosaic extent outward in
`aligned-extent` before merging. The padding is masked out and costs ~1% of area
(the Netherlands: +1317 x +3113 px, +1.10%).

`levels` tracks what `gdaladdo` will build — it stops once a level fits in 256 px
— so the padding stays proportionate on small countries instead of a fixed
power of two swallowing 12% of Luxembourg.

This was found only after the Netherlands shipped misaligned and needed a
10-hour re-mosaic. **Verify it, on the finished file:**

```python
# origin, in z17 pixels from the Mercator origin, must be an integer
ox = (gt[0] + 20037508.342789244) / 1.194328566955879
# and every overview ratio exactly 2.0, with dimensions divisible by 2^levels
```

Stronger still, check the fast path empirically — a reduced `ReadAsArray` must
come back **byte-identical** to the stored overview:

```python
via_full = band.ReadAsArray(ox, oy, 256*f, 256*f, buf_xsize=256, buf_ysize=256,
                            resample_alg=gdal.GRIORA_NearestNeighbour)
via_ov   = band.GetOverview(lvl).ReadAsArray(ox//f, oy//f, 256, 256)
assert np.array_equal(via_full, via_ov)
```

## 4. Resampling in the renderer

`src/render/layers/hillshading.rs` picks by ratio:

| case | algorithm | why |
|------|-----------|-----|
| served by an overview (ratio a power of 2, correct phase) | `NearestNeighbour` | returns stored pixels verbatim, 0.08 ms vs 16.5 ms for Lanczos, and the anti-aliasing is already baked into the `average` pyramid — HF energy within 0.5% |
| upscaling (overzoom) | `CubicSpline` | no negative lobes. Lanczos rings 18.1% at 8x and makes the output *sharper* than nearest; CubicSpline 0.04% |
| everything else | `Cubic` | Catmull-Rom, support 2 |

The power-of-two guard is load-bearing: `MAPRENDER_ALLOWED_SCALES=1,2,3,4` and
scale 3 yields ratios of 4/3, 8/3, 16/3, which are *not* overview-served.

**Erosion radius follows the kernel support**: 0 for Nearest, 2 for the
Cubic family (was a fixed 3 for Lanczos). That gives back 2–3 px of coastline at
every zoom below native. Use the *unclamped* window ratio — clamping it caused
edge seams.

Overviews are built `-r average`. That is what makes the Nearest fast path
legitimate rather than a shortcut.

## 5. Grid origin must be pinned, never derived

Deriving the window grid origin from the source extent was the Belgium trap:
adding a region moved the origin, every window id changed, and a resumable run
treated thousands of finished tiles as pending. Pin `x0`/`y0` at the country
extent floored to the step grid, and **never change them** — doing so renames
every tile.

## 6. Resumability traps

Everything in the pipeline skips on *existence*, not on freshness. If the inputs
change under a re-run, you must delete the downstream artefacts by hand:

- `contours-*.nu` prints `skip offset N — already done` and republishes the
  previous run's vectors from a stale `*_off*.gpkg` partial. This wasted a full
  Netherlands cycle; the only giveaway was an output byte-identical in size to
  the old one. Delete `<cc>_dem_2m.vrt`, `<cc>_dem_2m.tif` **and**
  `<cc>_contours_off*.gpkg` together.
- `shading run` skips the merge entirely once `out_tif` exists (deliberately —
  it refuses to merge a partial state), so re-merging needs the file renamed
  out of the way.

## 7. GDAL traps

- **`gdalbuildvrt` silently drops mismatched inputs.** A tile with the wrong
  band count is skipped behind a warning swallowed by the progress bar, punching
  an invisible hole in the mosaic. England lost two tiles that way for real.
  `lib/gdal.nu`'s `verify-vrt` counts sources and throws. When counting, dedupe:
  a tile appears once per band plus once for the mask (24034 tiles read as
  120170 `SourceFilename` entries).
- **`PREDICTOR=1` on the window DEM is load-bearing.**
  `feature-preserving-smoothing` does I/O via `wbgeotiff`, which ignores the TIFF
  Predictor tag (317) and decodes `PREDICTOR=2/3` float data as garbage (±Inf)
  **without erroring**.
- **`gdal_translate` has no `-te`.** Set the extent on the VRT with
  `gdalbuildvrt -te` instead.
- **Always run via `conda run -n geo`**, never with the env's `bin` on `PATH`.
  The latter leaves `PROJ_DATA` unset, degrades every CRS to `ENGCRS["unnamed"]`,
  and the warp fails hours in with "Cannot find coordinate operations".
- **Missing JXL codec is not a corrupt file.** A system `gdalinfo` without
  libjxl reports `Cannot open TIFF file due to missing codec JXL`. Always run the
  control test — open the *existing, working* file with the same binary — before
  concluding a transfer failed. This has produced a false alarm on both fm5 and
  fm6.

## 8. Datum hazards

Check `projinfo <src> EPSG:4326` before rendering. ETRS89-based systems
(25832/25833/3812/2169) reach 3857 through a null transform — nothing to get
wrong.

England is the cautionary case. EPSG:27700 is on OSGB36 and needs the OSTN15
NTv2 grid; PROJ does **not** error when it is missing, it silently falls back to
a 7-parameter Helmert. The shipped English products are ~1.9 m off true WGS84
(median 1.90 m, p95 3.57 m, max 4.67 m, and it varies spatially so no constant
offset corrects it). Shading and contours are consistently wrong *together*,
which is the better of the two bad states — so re-render both or neither.

## 9. PostGIS handoff — two production-breaking traps

**Run `pg_restore` as `freemap`, not as postgres.** `--no-owner` does not mean
"keep the owner", it means "assign to the restoring role". Restoring as the
postgres superuser yields a table owned by postgres with no ACL; the renderer
connects as `freemap` and every query fails with permission denied. Rendering
breaks with nothing in the journal. Fix: `ALTER TABLE ... OWNER TO freemap;`

**Pass the tablespace.** fm5 keeps contours in `contours_ts`, and
`--no-tablespaces` (required, because the dump carries this box's `osm_ext`)
silently drops the table onto `pg_default`, undoing the migration:

```
PGOPTIONS='-c default_tablespace=contours_ts' pg_restore --dbname=freemap \
  --no-owner --no-privileges --no-tablespaces --single-transaction dump
```

`splitter-rs` uses rust-postgres, which reads neither `$PGPASSWORD` nor
`~/.pgpass`. Use the **unix socket** URL — it peer-authenticates and needs no
password at all, which beats interpolating the secret into the environment:

```
DATABASE_URL="postgresql://martin@%2Fvar%2Frun%2Fpostgresql/martin"
```

Pass `--simplify-tolerance 2` explicitly; the default of 1.0 silently loads a
much denser table (Bayern: 1130 MB against 749 MB).

**Row counts prove nothing.** Before calling a deploy live, read as the serving
role and fetch a real tile:

```sql
SET ROLE freemap; SELECT count(*) FROM contours_xx WHERE wkb_geometry && ...;
```
```
curl -o /tmp/t.jpg -w '%{http_code}' http://127.0.0.1:4000/<z>/<x>/<y>
```

Deploy by renaming the live table aside rather than dropping it, so rollback is
two `ALTER TABLE`s.

## 10. Contour interval and what the renderer actually draws

`src/render/layers/contours.rs` filters by height: `% 50` at z12, `% 20` at
z13–14, `% 5` at z15+. A table built at a 5 m interval therefore shows its 5 m
lines only at z15 and above; before that filter was widened from `% 10`, a third
of the Dutch table (1.6 M rows) could not be drawn at any zoom.

Use 10 m unless the country is flat enough to need otherwise — the Netherlands
runs about −7 m to +50 m outside its one hilly corner, which is why it is the
exception.

## 11. Known open issues

- **Contours cross each other** (#99). The stored geometry is clean and
  `--simplify-tolerance 2` is not the cause; `ST_SimplifyVW` introduces crossings
  only at z12–13; at z16+ nothing simplifies, leaving
  `path_smooth_bezier_spline(..., 1.0)` — AGG `smooth_poly1` at maximum
  smoothing, which overshoots on sharp vertices and is applied per-line, so
  neighbours can bulge across each other.
- **Holes over dense canopy** (#100). AHN5 only; the fill reach of 5–6 m closes
  98–100% of forest voids at most sites but leaves 1.3–1.8% in dense conifer
  plantations.
- **Edge artefacts at internal state borders.** Rendering a German state in
  isolation means `-compute_edges` extrapolates where a window has no neighbour
  across the boundary. The fix is to include neighbouring states' tiles in the
  VRT as *context* while still writing only tiles inside this extent — cheap,
  but it needs the neighbours downloaded first.
