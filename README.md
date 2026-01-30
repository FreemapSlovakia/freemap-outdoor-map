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

## Land polygons

```sh
wget https://osmdata.openstreetmap.de/download/land-polygons-complete-3857.zip

unzip land-polygons-complete-3857.zip

ogr2ogr \
  -f PostgreSQL \
  PG:"host=localhost dbname=osm_db user=osm_user password=pw" \
  land-polygons-complete-3857 \
  -nln land_polygons_raw \
  -lco GEOMETRY_NAME=geom \
  -lco FID=osm_id \
  -lco SPATIAL_INDEX=GIST \
  -t_srs EPSG:3857 \
  -nlt PROMOTE_TO_MULTI \
  -overwrite

psql < sql/land-polygons.sql
```

## Peak isolations

TBD

Legacy manual: https://github.com/FreemapSlovakia/freemap-mapnik/blob/develop/doc/PEAK_ISOLATION.md

## Contours and shaded relief

TBD

Legacy manual: https://github.com/FreemapSlovakia/freemap-mapnik/blob/develop/doc/SHADING_AND_CONTOURS.md

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
