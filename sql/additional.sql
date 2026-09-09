-- Access value that forbids passage. Anything unknown counts as forbidden, matching how
-- the renderer has always read these tags.
CREATE OR REPLACE FUNCTION access_denied(value text) RETURNS boolean
  LANGUAGE sql IMMUTABLE PARALLEL SAFE
AS $$
  SELECT value NOT IN ('', 'yes', 'designated', 'official', 'permissive')
$$;

-- Restriction bitmask of a road: 1 = no bicycle, 2 = no foot. The `road_access_restrictions`
-- layer compares a way's mask with its neighbours' to find where a restriction begins, so
-- the rules must be expressible for any way, not just for the ones being drawn.
CREATE OR REPLACE FUNCTION road_restriction(access text, vehicle text, bicycle text, foot text)
  RETURNS smallint
  LANGUAGE sql IMMUTABLE PARALLEL SAFE
AS $$
  SELECT (
    CASE
      WHEN access_denied(bicycle)
        OR (bicycle = '' AND access_denied(vehicle))
        OR (bicycle = '' AND vehicle = '' AND access_denied(access))
      THEN 1 ELSE 0
    END
    +
    CASE
      WHEN access_denied(foot)
        OR (foot = '' AND access_denied(access))
      THEN 2 ELSE 0
    END
  )::smallint
$$;

CREATE TABLE IF NOT EXISTS isolations (
  osm_id BIGINT PRIMARY KEY,
  dem_ele REAL NOT NULL,
  isolation REAL NOT NULL
);

-- not sure if all those indexes help ;-)
--
CREATE INDEX CONCURRENTLY admin_relations_level ON osm_admin_relations (admin_level);

CREATE INDEX CONCURRENTLY admin_members_member ON osm_admin_members (member);

CREATE INDEX CONCURRENTLY idx_colour ON osm_routes (colour);

CREATE INDEX CONCURRENTLY idx_symbol ON osm_routes ("osmc:symbol");

CREATE INDEX CONCURRENTLY idx_network ON osm_routes (network);

CREATE INDEX CONCURRENTLY idx_type ON osm_routes (type);

CREATE INDEX CONCURRENTLY osm_pois_osm_id ON osm_pois (osm_id);

CREATE INDEX CONCURRENTLY osm_pois_type ON osm_pois (type);

CREATE INDEX CONCURRENTLY osm_landcovers_type ON osm_landcovers (type);

CREATE INDEX CONCURRENTLY osm_places_type ON osm_places (type);

CREATE INDEX CONCURRENTLY osm_places_name ON osm_places (name);

CREATE INDEX CONCURRENTLY osm_route_members_idx1 ON osm_route_members (member);

CREATE INDEX CONCURRENTLY osm_route_members_idx2 ON osm_route_members (type);

CREATE INDEX CONCURRENTLY osm_route_members_type_member_idx ON osm_route_members (type, member);

CREATE INDEX CONCURRENTLY osm_route_members_idx1_g1 ON osm_route_members_gen1(member);

CREATE INDEX CONCURRENTLY osm_route_members_idx2_g1 ON osm_route_members_gen1(type);

CREATE INDEX CONCURRENTLY osm_route_members_idx1_g0 ON osm_route_members_gen0(member);

CREATE INDEX CONCURRENTLY osm_route_members_idx2_g0 ON osm_route_members_gen0(type);

CREATE INDEX CONCURRENTLY osm_shops_type ON osm_shops(type);

CREATE INDEX CONCURRENTLY osm_feature_lines_type ON osm_feature_lines(type);

CREATE INDEX CONCURRENTLY osm_pois_peak_named_geom_gist ON osm_pois USING GIST (geometry) WHERE type = 'peak' AND name <> '';
