-- Rebuild the four per-zoom land (coastline) tables from land_polygons_raw.
--
-- land_polygons_raw comes from osmdata.openstreetmap.de's land-polygons-complete
-- shapefile (see scripts/reimport-land-polygons.nu). Nothing queries it at
-- runtime; only the land_z* tables below are read, one per zoom band, by
-- src/render/layers/sea.rs.
--
-- BUILD INTO *_new, THEN SWAP BY RENAME. The obvious form of this script --
-- one transaction that drops each land_z* and recreates it in place -- holds an
-- ACCESS EXCLUSIVE lock on land_z5_7 from its DROP until COMMIT, i.e. for the
-- whole rebuild. Every sea query at that zoom band blocks for those tens of
-- minutes, which with MAPRENDER_GLOBAL_TIMEOUT_SECS is timed-out tiles rather
-- than merely slow ones. Building under a temporary name instead keeps the
-- exclusive lock down to the milliseconds of the final rename, at the cost of
-- holding both generations of the tables (~3 GB extra) until the swap.
--
-- Each statement runs in its own transaction (psql autocommit), deliberately:
-- the new tables must be committed and visible before the swap, and a failure
-- part-way leaves the live tables untouched. Re-running is safe -- the leftover
-- *_new tables are dropped up front.
--
-- Run with ON_ERROR_STOP so a failed build never reaches the swap:
--   psql -v ON_ERROR_STOP=1 -f sql/land-polygons.sql


-- ============================================================
-- Z5-Z7  (150 m simplify, 512 vertices)
-- ============================================================

DROP TABLE IF EXISTS land_z5_7_new;

CREATE TABLE land_z5_7_new AS
SELECT
  ST_Subdivide(
    ST_SimplifyPreserveTopology(geom, 150),
    512
  ) AS geometry
FROM land_polygons_raw
WHERE geom IS NOT NULL;

CREATE INDEX land_z5_7_new_geometry_gix
  ON land_z5_7_new
  USING GIST (geometry);

ANALYZE land_z5_7_new;



-- ============================================================
-- Z8-Z10  (30 m simplify, 512 vertices)
-- ============================================================

DROP TABLE IF EXISTS land_z8_10_new;

CREATE TABLE land_z8_10_new AS
SELECT
  ST_Subdivide(
    ST_SimplifyPreserveTopology(geom, 30),
    512
  ) AS geometry
FROM land_polygons_raw
WHERE geom IS NOT NULL;

CREATE INDEX land_z8_10_new_geometry_gix
  ON land_z8_10_new
  USING GIST (geometry);

ANALYZE land_z8_10_new;



-- ============================================================
-- Z11-Z13  (4 m simplify, 512 vertices)
-- ============================================================

DROP TABLE IF EXISTS land_z11_13_new;

CREATE TABLE land_z11_13_new AS
SELECT
  ST_Subdivide(
    ST_SimplifyPreserveTopology(geom, 4),
    512
  ) AS geometry
FROM land_polygons_raw
WHERE geom IS NOT NULL;

CREATE INDEX land_z11_13_new_geometry_gix
  ON land_z11_13_new
  USING GIST (geometry);

ANALYZE land_z11_13_new;



-- ============================================================
-- Z14+  (NO simplification, 512 vertices)
-- ============================================================

DROP TABLE IF EXISTS land_z14_plus_new;

CREATE TABLE land_z14_plus_new AS
SELECT
  ST_Subdivide(
    geom,
    512
  ) AS geometry
FROM land_polygons_raw
WHERE geom IS NOT NULL;

CREATE INDEX land_z14_plus_new_geometry_gix
  ON land_z14_plus_new
  USING GIST (geometry);

ANALYZE land_z14_plus_new;



-- ============================================================
-- Swap: all four tables at once, in one short transaction
-- ============================================================

BEGIN;

-- Fail fast instead of queueing behind a long-running reader: while we wait for
-- ACCESS EXCLUSIVE, every query arriving after us queues behind us too, so a
-- patient wait here is exactly the stall this script exists to avoid. On timeout
-- nothing is lost -- the *_new tables survive, so just re-run the swap.
SET LOCAL lock_timeout = '5s';

DROP TABLE IF EXISTS land_z5_7;
ALTER TABLE land_z5_7_new RENAME TO land_z5_7;
ALTER INDEX land_z5_7_new_geometry_gix RENAME TO land_z5_7_geometry_gix;

DROP TABLE IF EXISTS land_z8_10;
ALTER TABLE land_z8_10_new RENAME TO land_z8_10;
ALTER INDEX land_z8_10_new_geometry_gix RENAME TO land_z8_10_geometry_gix;

DROP TABLE IF EXISTS land_z11_13;
ALTER TABLE land_z11_13_new RENAME TO land_z11_13;
ALTER INDEX land_z11_13_new_geometry_gix RENAME TO land_z11_13_geometry_gix;

DROP TABLE IF EXISTS land_z14_plus;
ALTER TABLE land_z14_plus_new RENAME TO land_z14_plus;
ALTER INDEX land_z14_plus_new_geometry_gix RENAME TO land_z14_plus_geometry_gix;

COMMIT;
