# Freemap Outdoor Map

Reimplementation of https://github.com/FreemapSlovakia/freemap-mapnik in Rust.

## Why?

- [Mapnik](https://github.com/mapnik/mapnik/) is no longer actively developed, except for keeping it building with recent toolchains.
- Full control over rendering
- Much lower resource usage (CPU, memory)

## Technical details

- Uses PostGIS for data
- Uses Cairo for rendering
- Uses GDAL to read GeoTIFFs

## Create database

Setup DB environment variables. We will use them later at other places too:

```sh
export PGDATABASE=...
export PGPASSWORD=...
export PGUSER=...
```

Create new postgres database and initialize it as DB superuser with [initial.sql](./sql/initial.sql):

```sh
sudo -u postgres psql < sql/initial.sql
```

## Land polygons (coastlines)

Coastlines do not come from OSM via imposm — `mapping.yaml` has no `natural=coastline`.
They come from [osmdata.openstreetmap.de](https://osmdata.openstreetmap.de/)'s
pre-assembled land-polygons shapefile, rebuilt from the planet daily, which is already a
valid polygon set (the raw coastline ways are not). Re-import it with
[scripts/reimport-land-polygons.nu](./scripts/reimport-land-polygons.nu):

```sh
nu scripts/reimport-land-polygons.nu
```

It downloads and extracts the shapefile, loads it into `land_polygons_raw` with `ogr2ogr`,
then runs [sql/land-polygons.sql](./sql/land-polygons.sql) to derive the four per-zoom
tables the renderer reads (`land_z5_7`, `land_z8_10`, `land_z11_13`, `land_z14_plus` — see
`src/render/layers/sea.rs`). On fm5 this measured 9 s to download, 36 s to load and ~40 min
to rebuild — so budget ~45 min, ~3 GB of scratch space for the shapefile and ~7 GB free in
the tablespace. The download is resumable and the whole thing is safe to re-run.

**Rendering keeps serving throughout, and no restart is needed afterwards.** `land_polygons_raw`
is build input only, never queried at runtime, and the SQL builds `land_z*_new` and swaps all
four in by rename in one short transaction at the end — so the `ACCESS EXCLUSIVE` lock lasts
milliseconds instead of the tens of minutes an in-place `DROP`/`CREATE` would hold it for.
The cost is holding both generations of the tables (~3 GB extra) until the swap. If only the
swap fails (it uses a 5 s `lock_timeout` rather than queueing behind a slow reader), the built
`land_z*_new` tables survive — re-run with `--sql-only`, or apply the swap block by hand.

On a host where the database roles use peer authentication and the tables are owned by another
unix user, run the script as yourself and let `--db-user` hand `psql` and `ogr2ogr` over via
`sudo`; the SQL goes in on stdin, so only the shapefile has to be readable by that user. On fm5:

```sh
scp scripts/reimport-land-polygons.nu sql/land-polygons.sql fm5:land-polygons-reimport/

ssh fm5 '~/.cargo/bin/nu land-polygons-reimport/reimport-land-polygons.nu \
           --db-user freemap --database freemap \
           --sql land-polygons-reimport/land-polygons.sql'
```

## Peak isolations

TBD

Legacy manual: https://github.com/FreemapSlovakia/freemap-mapnik/blob/develop/doc/PEAK_ISOLATION.md

## Contours and shaded relief

~~Legacy manual: https://github.com/FreemapSlovakia/freemap-mapnik/blob/develop/doc/SHADING_AND_CONTOURS.md~~

Run [scripts/shading.nu](./scripts/shading.nu) to produce `shading.tif`. Adjust `ZOOM`, `PARALLEL`, and the whitebox_tools parameters at the top of the script.

```sh
nu scripts/shading.nu
```

The script is resumable — re-running it skips already completed tiles.

### GDAL with JPEG-XL-in-GeoTIFF

The merge step uses `-co COMPRESS=JXL`, and this binary reads the resulting `shading.tif` at runtime. Both require GDAL linked against a libtiff that has libjxl support.

Check the system GDAL:

```sh
gdalinfo --format GTiff | grep -i jxl
```

If JXL doesn't appear there, the system GDAL can't do it. As of Debian trixie/forky, `libgdal38` links libjxl (so the standalone `.jxl` driver works), but `libtiff6` does not — so `COMPRESS=JXL` inside a GeoTIFF is unavailable. Install GDAL via mamba/miniforge instead:

```sh
mamba create -n geo -c conda-forge gdal libtiff
mamba activate geo
gdalinfo --format GTiff | grep -i jxl   # should print "JXL"
```

For `cargo build` to link against this GDAL, put a `.cargo/config.toml` at the repo root pointing at the env (adjust the path to your miniforge install):

```toml
[build]
rustflags = ["-C", "link-arg=-Wl,-rpath,/home/<you>/miniforge3/envs/geo/lib"]

[env]
PKG_CONFIG_PATH = "/home/<you>/miniforge3/envs/geo/lib/pkgconfig"
```

For `scripts/shading.nu`, invoke `gdal_translate`/`gdaladdo` from the env (e.g. `mamba activate geo` before running, or hard-code `~/miniforge3/envs/geo/bin/...` paths).

## Country labels

Import hand-crafted country labels:

```sh
psql < sql/country-names.sql
```

## Geonames

Import hand-crafted geonames (e.g., mountain range names):

```sh
psql < sql/geonames.sql
```

## Country borders

Geofabrik extracts don't contain complete borders for the countries we need. Therefore, we import all country borders from `planet.osm.pbf`:

```sh
# fast-download planet file (use wget if you are poor)
aria2c -x 16 https://planet.osm.org/pbf/planet-latest.osm.pbf

# extract country boundaries
osmium tags-filter -t -o admin_level_2_with_refs.osm.pbf planet-251215.osm.pbf r/admin_level=2
osmium tags-filter -o boundary_admin_level_2_with_refs.osm.pbf admin_level_2_with_refs.osm.pbf r/boundary=administrative
osmium tags-filter -R -i -o boundary_admin_level_not2_with_garbage.osm.pbf boundary_admin_level_2_with_refs.osm.pbf r/admin_level=2
osmium cat -t relation -o boundary_admin_level_not2.osm.pbf boundary_admin_level_not2_with_garbage.osm.pbf
osmium removeid -I boundary_admin_level_not2.osm.pbf -o country_borders_with_garbage.osm.pbf boundary_admin_level_2_with_refs.osm.pbf
osmium tags-filter -o country_borders.osm.pbf country_borders_with_garbage.osm.pbf r/admin_level=2

# import country boundaries
imposm import -connection postgis: -mapping borders.yaml -read countries.osm.pbf -write -overwritecache
imposm import -connection postgis: -mapping borders.yaml -deployproduction
```

## Country polygons

Needed only for `--place-type-overrides` (`MAPRENDER_PLACE_TYPE_OVERRIDES`), which decides
per country how a `place=*` type is labelled. The polygons are built from the country
borders imported above, so no extra download is needed — but that import must have
deployed both of its tables, `osm_country_members` (the border ways) and
`osm_country_relations` (their ISO 3166-1 codes):

```sh
psql < sql/countries.sql
```

[countries.sql](./sql/countries.sql) polygonizes each relation's border ways, cuts out
enclaves (Lesotho, San Marino, …), tags every polygon with the relation's lowercase
ISO 3166-1 code and subdivides them — the renderer does a point-in-polygon lookup per
place label, and whole-country polygons would make it slow. It takes about half a minute
and results in ~50 000 polygons for ~215 countries. Re-run it after re-importing borders.

## Place labels

`place=*` is tagged at very different granularities per country, so one zoom ladder makes
some countries far busier than others. [doc/place-labels.md](./doc/place-labels.md) explains
the `MAPRENDER_PLACE_TYPE_OVERRIDES` rule syntax, how to measure a country before writing a
rule, and what the measurements said — with the per-country data in
[doc/place-density.csv](./doc/place-density.csv).

## Importing OSM data

⚠️ You must use [Imposm with improvements](https://github.com/FreemapSlovakia/imposm3).

Import OSM data:

```sh
imposm import \
  -connection postgis: \
  -mapping mapping.yaml \
  -read europe-latest.osm.pbf \
  -diff \
  -write \
  -cachedir ./cache \
  -diffdir ./diff \
  -overwritecache \
  -limitto limit-europe.geojson \
  -limittocachebuffer 10000 \
  -optimize
```

\* includes arguments that enable (eg minutely) updates

Deploy the import:

```sh
imposm import \
  -connection postgis: \
  -mapping mapping.yaml \
  -deployproduction
```

Now import [additional.sql](./sql/additional.sql):

```sh
psql < sql/additional.sql
```

## Fonts

Install fonts referenced from [fonts.conf](./fonts.conf) and upon running `freemap-outdoor-map` set its pathname to environment variable `FONTCONFIG_FILE`.

## Running

Install Rust and build+install the app:

```sh
cargo install --path .
```

Configure environment variables or pass configuration as commandline arguments to `freemap-outdoor-map`. Run `freemap-outdoor-map --help` for details.

For environment variables you can use `.env` file. See [.env.sample](./.env.sample).

## Nginx

For production it is advisable to use a proxy server.
For Nginx you can find configuration in [outdoor.tiles.freemap.sk](./etc/nginx/sites-available/outdoor.tiles.freemap.sk).

It proxies tiles rather than serving them off disk with `try_files`, and runs with
`MAPRENDER_SERVE_CACHED=true`. The renderer is the only thing that can read a tile's
attribution out of its `COM` segment, so a tile served past it arrives without its
`Server-Timing` (see [Attribution](#attribution)) — and with `serve_cached` the renderer
handles the cache hit and the miss in one place, so the proxy needs no `try_files`
fallback and no `?rerender` special case. The cost is that a cache hit goes through the
renderer's read instead of `sendfile`.

`Server-Timing`'s `src` metric says which of the two a response was, so cache behaviour is
still visible from `curl -I` — and now from the browser too.

## Systemd service

In production, freemap-outdoor-map should run as a system service.
You can use [freemap-outdoor-map.service](./etc/system/systemd/freemap-outdoor-map.service) systemd unit file.
For Imposm3 see [imposm.service](./etc/system/systemd/imposm.service).

## API

### TMS

"TMS" URL template:

`http://localhost:3050/{zoom}/{x}/{y}@{scale}x`

### Map export

Request:

<details>
<summary>POST /export</summary>

```http
POST /export
Content-Type: application/json

{
  "bbox": [
    20.973758697509766,
    48.749454680489244,
    21.086025238037113,
    48.81325072203008
  ],
  "zoom": 14,
  "format": "jpeg",
  "scale": 3.125,
  "features": {
    "shading": true,
    "contours": true,
    "hikingTrails": true,
    "bicycleTrails": true,
    "skiTrails": true,
    "horseTrails": true,
    "featureCollection": {
      "type": "FeatureCollection",
      "features": [
        {
          "type": "Feature",
          "properties": {
            "name": "Yay!",
            "color": "#1100ff",
            "width": 4
          },
          "geometry": {
            "type": "LineString",
            "coordinates": [
              [
                21.031780242919922,
                48.77615934438715
              ],
              [
                21.043024063110355,
                48.7859437268498
              ]
            ]
          }
        }
      ]
    }
  }
}

```

</details>
<br>
Response:

```http
200 OK
Content-Type: aplication/json

{"token":"6f41b0ebf3bef99cad07c1041fac3339"}
```

**Waiting for export:**

Request:

```http
HEAD /export?token=6f41b0ebf3bef99cad07c1041fac3339
```

Responds with 200 OK if ready or times out if still exporting.

**Downloading export:**

```http
GET /export?token=6f41b0ebf3bef99cad07c1041fac3339
```

**Deleting export:**

```http
DELETE /export?token=6f41b0ebf3bef99cad07c1041fac3339
```

A client that has taken its file deletes the job. A finished job that nobody deletes is
dropped along with its temporary file `--export-retention-secs`
(`MAPRENDER_EXPORT_RETENTION_SECS`, 900 by default) after it finished.

### WMTS

Endpoint: `/service`

### Attribution

Every rendered tile carries the datasets that contributed a pixel to it, as a JPEG `COM`
segment (`FF FE | len_hi len_lo | payload`) placed right after the JFIF `APP0`, which keeps
the file a conformant JFIF. Reading it back walks the segment chain over the first kilobyte
— no fixed offset to depend on which `APP` segments the encoder writes, and no decoding.

The API names a dataset by a namespaced code — `osm`, `shading:<key>`, `contours:<key>`,
where `<key>` is a `--hillshading-hierarchy` / `--contour-countries` key or `_` for a global
fallback source. Codes are namespaced because a region's shading and its contours can come
from different sources under different licences.

The stored payload is the same list with the namespace shortened to one character, so it
stays small on every cached tile: 9 bytes for a tile inside Slovakia, 31 for the nine
sources of a tile on a triple border. The keys go in verbatim — they are the configuration's
own identifiers, so there is no second numbering that could drift from the tiles already on
disk, and they cannot contain the separator because `--hillshading-hierarchy` and
`--contour-countries` split on `,` themselves.

Codes are sorted by their long form, so the namespaces group and a given set of sources
always encodes to the same bytes:

```
csk,o,ssk                        →  contours:sk, osm, shading:sk
c_,cat,ccz,csk,o,s_,sat,scz,ssk  →  osm and eight terrain sources
```

Read `sat` as `s` + `at` (Austrian shading), not as a word — every code is one namespace
character followed by the key.

Which datasets a tile credits is decided from the resampled masks and shading surfaces the
render itself used, in the same coordinate space as the pixels being credited: three pixels
of Czech shading inside Slovakia are three bits of the `cz` mask, and both sources are
credited. Two things it does not see, both of which can only over-credit: the dry-land clip
the whole layer is drawn under, so a dataset whose only pixels on a tile fall on water is
still named; and whether a contour line really falls inside the region a country's contours
may draw in, rather than just that the region and the rows both exist.

Exported PNG and PDF carry the same list — in a `tEXt` chunk keyed `map-attribution`, and in
the document keywords.

**For live browsing**, every tile response also carries the same short codes in a header:

```http
Server-Timing: src;desc="cache", attr;desc="csk o ssk"
Timing-Allow-Origin: *
```

`Server-Timing` is the one response header JavaScript can read off an `<img>`, through
`PerformanceResourceTiming.serverTiming`, so a page credits exactly the datasets painted in
front of it without fetching tile bytes or taking over the tile lifecycle. Without
`Timing-Allow-Origin` a cross-origin page gets an empty `serverTiming` and no error anywhere,
so it goes on every tile response.

Metrics are comma-separated, which is why the code list inside `attr` is not:

- `src` is `cache`, `render` or `outside-coverage` — where the body came from.
- `attr` is the codes, present exactly when they are known: on a fresh render, on a cache
  hit, on the `304` of a revalidated tile, and on the out-of-coverage gray tile, where
  `desc=""` says "nothing to credit" rather than "unknown". A tile cached before tiles
  carried attribution gets no `attr` at all, and turns over on its own.

The `COM` segment is storage — it is what a cached tile carries its codes in between
renders, and what an exported file carries with it. The header is delivery.

**Code dictionary:**

```http
GET /licenses
```

Resolves each code to the sources behind it — a list, because one dataset can be several
models (Belgium's relief is two):

```json
{
  "osm": [{ "title": "© OpenStreetMap contributors", "url": "https://osm.org/copyright" }],
  "shading:sk": [{ "title": "DMR 5.0: ÚGKK SR", "url": "…" }],
  "contours:sk": [{ "title": "DMR 5.0: ÚGKK SR", "url": "…" }]
}
```

`osm` is built in. Everything else comes from each dataset's own `attribution.json`, next to
its `final.tif` under `--hillshading-base-path`:

```json
{
  "covers": ["shading", "contours"],
  "sources": [{ "title": "DMR 5.0: ÚGKK SR", "url": "https://…" }]
}
```

The licence belongs with the data: the script that downloads a DEM knows its terms, adding
a dataset is creating its directory, and deleting one takes its licence with it — there is
no central list to drift. `covers` says which namespaces these sources answer for, and
defaults to `["shading"]` alone: contours normally come from this very DEM, but a region
that took them from elsewhere must not silently credit this dataset for them. Leaving a code
unresolved is the lesser error, and the server names it at startup either way:

```
attribution: nothing resolves contours:pl — add "contours" to `covers` in the pl dataset's
             attribution.json, or list the code in --licenses
```

[scripts/write-dtm-attribution.nu](./scripts/write-dtm-attribution.nu) writes these files
for every dataset present, and names any it has no entry for:

```sh
nu scripts/write-dtm-attribution.nu /fm/data2/hillshading
```

`--licenses` (`MAPRENDER_LICENSES`) is optional on top: a JSON file of the same
`code -> sources` shape that replaces whatever a dataset said, for a code with no dataset
directory or a correction that should not touch the data volume.

Titles are not localized — they are the rights-holder's own attribution string and the
licence's name — so one `ETag` covers the document for every client. It is served with
`Cache-Control: no-cache`, so a client revalidates and a newly added dataset never leaves it
holding a code it cannot resolve.

**Export attribution:** an export's codes come back on the poll the client already makes, in
the `X-Attribution` response header of `HEAD /export` and `GET /export` — the same short
spelling as the tile header and the embedded metadata, `o,ssk,csk`.

## Notes

Buffer polygon for imposm:

```sh
ogr2ogr -f GeoJSON limit-europe-buffered.geojson limit-europe.geojson \
  -dialect sqlite \
  -sql "WITH P AS (SELECT BufferOptions_SetJoinStyle('MITRE') AS a, BufferOptions_SetMitreLimit(5.0) AS b) SELECT ST_Transform(ST_Buffer(ST_Transform(geometry, 3857), 10000), 4326) AS geometry, * FROM \"limit-europe\", P"
```
