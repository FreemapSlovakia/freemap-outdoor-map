-- Run after loading the `geonames` table from scripts/geonames.py output (see README).

DROP MATERIALIZED VIEW IF EXISTS geonames_smooth;

CREATE MATERIALIZED VIEW geonames_smooth AS
SELECT
  ogc_fid,
  ST_ChaikinSmoothing(geometry, 3)::geometry(LineString, 3857) AS geometry,
  name,
  minzoom,
  maxzoom
FROM geonames;

CREATE UNIQUE INDEX geonames_smooth_pk
ON geonames_smooth (ogc_fid);

CREATE INDEX geonames_smooth_geometry_idx
ON geonames_smooth
USING GIST (geometry);
