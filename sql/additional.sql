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

-- The graded-way overlays each select one sparsely tagged column out of ~1M
-- roads, so without these every overlay tile scans every road in its bbox. The
-- predicate has to match the renderer's `WHERE` exactly (see
-- `render::draw::graded_dots`): a cast around the column — `NULLIF(x,'')::int
-- > 0` — is not something the planner can prove implies `x <> ''`, and it falls
-- back to the plain geometry index.
CREATE INDEX CONCURRENTLY osm_roads_sac_scale_geom_gist ON osm_roads USING GIST (geometry) WHERE sac_scale > 0;

CREATE INDEX CONCURRENTLY osm_roads_smoothness_geom_gist ON osm_roads USING GIST (geometry) WHERE smoothness > 0;

CREATE INDEX CONCURRENTLY osm_roads_piste_difficulty_geom_gist ON osm_roads USING GIST (geometry) WHERE piste_difficulty > 0;

CREATE INDEX CONCURRENTLY osm_roads_mtb_scale_geom_gist ON osm_roads USING GIST (geometry) WHERE mtb_scale <> '';

CREATE INDEX CONCURRENTLY osm_roads_via_ferrata_scale_geom_gist ON osm_roads USING GIST (geometry) WHERE via_ferrata_scale <> '';
