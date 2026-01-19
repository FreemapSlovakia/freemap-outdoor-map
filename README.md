# RustMap

Reimplementation of https://github.com/FreemapSlovakia/freemap-mapnik into Rust, helping Mapnik to rest in peace.

## Why?

- Mapnik is no more actively developed except for keeping it to build itself with tools of the recent versions.
- Better control of the rendering
- Improve resource demands (CPU, memory)

## Technical details

- uses the same PostGIS schema as freemap-mapnik
- uses Cairo for rendering
- uses GDAL to read from GeoTIFFs

## Running

You must install Rust.

Configure [.env](./.env) and run `cargo run`.

TMS URL is then `http://localhost:3050/{zoom}/{x}/{y}@2x` (adjust your scaling).

## Land polygons

```sh
wget https://osmdata.openstreetmap.de/download/land-polygons-complete-3857.zip
unzip land-polygons-complete-3857.zip
ogr2ogr \
  -f PostgreSQL \
  PG:"host=localhost dbname=martin user=martin password=b0n0" \
  land-polygons-complete-3857 \
  -nln land_polygons_raw \
  -lco GEOMETRY_NAME=geom \
  -lco FID=osm_id \
  -lco SPATIAL_INDEX=GIST \
  -t_srs EPSG:3857 \
  -nlt PROMOTE_TO_MULTI \
  -overwrite
```

## Country borders

```sh
aria2c -x 16 https://planet.osm.org/pbf/planet-latest.osm.pbf
osmium tags-filter -t -o admin_level_2.osm.pbf planet-251215.osm.pbf r/admin_level=2
borders-tool  make-borders planet-251215.osm.pbf countries.osm.pbf
```

open countries.osm.pbf and download missing members, then

```sh
imposm import -connection postgis://martin:b0n0@localhost/martin -mapping ../borders.yaml -read ~/hs/countries2.osm.pbf -write -overwritecache
imposm import -connection postgis://martin:b0n0@localhost/martin -mapping ../borders.yaml -deployproduction
```
