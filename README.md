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

### WMTS

Endpoint: `/service`

## Notes

Buffer polygon for imposm:

```sh
ogr2ogr -f GeoJSON limit-europe-buffered.geojson limit-europe.geojson \
  -dialect sqlite \
  -sql "WITH P AS (SELECT BufferOptions_SetJoinStyle('MITRE') AS a, BufferOptions_SetMitreLimit(5.0) AS b) SELECT ST_Transform(ST_Buffer(ST_Transform(geometry, 3857), 10000), 4326) AS geometry, * FROM \"limit-europe\", P"
```
