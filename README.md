# RustMap

Reimplementation of https://github.com/FreemapSlovakia/freemap-mapnik into Rust, helping Mapnik to rest in peace.

## Why?

- Mapnik is no more actively developed except for keeping it to build itself with tools of the recent versions.
- Better control of the rendering
- Massively improve resource demands (CPU, memory)

## Technical details

- uses PostGIS for data
- uses Cairo for rendering
- uses GDAL to read from GeoTIFFs

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
```

## Import country borders

TODO: try to replace `borders-tool` and JOSM steps

```sh
aria2c -x 16 https://planet.osm.org/pbf/planet-latest.osm.pbf
osmium tags-filter -t -o admin_level_2.osm.pbf planet-251215.osm.pbf r/admin_level=2
borders-tool make-borders planet-251215.osm.pbf countries.osm.pbf
```

Now open countries.osm.pbf in JOSM and download missing members and save it. Follow:

```sh
imposm import -connection postgis://osm_db:pw@localhost/osm_db -mapping borders.yaml -read countries.osm.pbf -write -overwritecache
imposm import -connection postgis://osm_db:pw@localhost/osm_db -mapping borders.yaml -deployproduction
```

## Importing OSM data

You must use [Imposm with improvements](https://github.com/FreemapSlovakia/imposm3).

TODO document. For now see https://github.com/FreemapSlovakia/freemap-mapnik/blob/develop/doc/INSTALL.md but ignore Nodejs stuff.

## Fonts

TODO

## Running

Install Rust and build+install the app:

```sh
cargo install --path .
```

Configure env variables (you can use `.env` file) or pass arguments to `maprender`. Run `maprender --help` for details.

TMS URL is then `http://localhost:3050/{zoom}/{x}/{y}@2x` (adjust your scaling).
