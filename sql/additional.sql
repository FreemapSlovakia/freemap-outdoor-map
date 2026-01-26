CREATE TABLE IF NOT EXISTS isolations (
  osm_id BIGINT PRIMARY KEY,
  lon FLOAT,
  lat FLOAT,
  isolation INT NOT NULL
);

-- not sure if those indexes help ;-)
--
CREATE INDEX admin_relations_level ON osm_admin_relations (admin_level);

CREATE INDEX admin_members_member ON osm_admin_members (member);

CREATE INDEX idx_colour ON osm_routes (colour);

CREATE INDEX idx_symbol ON osm_routes ("osmc:symbol");

CREATE INDEX idx_network ON osm_routes (network);

CREATE INDEX idx_type ON osm_routes (type);

CREATE INDEX osm_features_osm_id ON osm_features (osm_id);

CREATE INDEX osm_features_type ON osm_features (type);

CREATE INDEX osm_places_type ON osm_places (type);

CREATE INDEX osm_route_members_idx1 ON osm_route_members (member);

CREATE INDEX osm_route_members_idx2 ON osm_route_members (type);

create index osm_route_members_idx1_g1 on osm_route_members_gen1(member);

create index osm_route_members_idx2_g1 on osm_route_members_gen1(type);

create index osm_route_members_idx1_g0 on osm_route_members_gen0(member);

create index osm_route_members_idx2_g0 on osm_route_members_gen0(type);

-- There seems to be a bug in imposm3. Workaround by using a trigger.
-- https://github.com/omniscale/imposm3/issues/293

CREATE OR REPLACE FUNCTION osm_route_members_insert_trigger()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO osm_route_members_gen1 (osm_id, member, role, type, geometry)
    VALUES (NEW.osm_id, NEW.member, NEW.role, NEW.type, ST_SimplifyPreserveTopology(NEW.geometry, 50));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION osm_route_members_update_trigger()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE osm_route_members_gen1
    SET member = NEW.member,
        role = NEW.role,
        type = NEW.type,
        geometry = ST_SimplifyPreserveTopology(NEW.geometry, 50)
    WHERE osm_id = NEW.osm_id AND type = NEW.type;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;


CREATE OR REPLACE FUNCTION osm_route_members_delete_trigger()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM osm_route_members_gen1
    WHERE osm_id = OLD.osm_id AND type = OLD.type;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER osm_route_members_after_insert
AFTER INSERT ON osm_route_members
FOR EACH ROW
EXECUTE FUNCTION osm_route_members_insert_trigger();

CREATE OR REPLACE TRIGGER osm_route_members_after_update
AFTER UPDATE ON osm_route_members
FOR EACH ROW
EXECUTE FUNCTION osm_route_members_update_trigger();

CREATE OR REPLACE TRIGGER osm_route_members_after_delete
AFTER DELETE ON osm_route_members
FOR EACH ROW
EXECUTE FUNCTION osm_route_members_delete_trigger();


CREATE OR REPLACE FUNCTION osm_route_members_gen0_insert_trigger()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO osm_route_members_gen0 (osm_id, member, role, type, geometry)
    VALUES (NEW.osm_id, NEW.member, NEW.role, NEW.type, ST_SimplifyPreserveTopology(NEW.geometry, 200));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION osm_route_members_gen0_update_trigger()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE osm_route_members_gen0
    SET member = NEW.member,
        role = NEW.role,
        type = NEW.type,
        geometry = ST_SimplifyPreserveTopology(NEW.geometry, 200)
    WHERE osm_id = NEW.osm_id AND type = NEW.type;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION osm_route_members_gen0_delete_trigger()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM osm_route_members_gen0
    WHERE osm_id = OLD.osm_id AND type = OLD.type;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER osm_route_members_gen0_after_insert
AFTER INSERT ON osm_route_members
FOR EACH ROW
EXECUTE FUNCTION osm_route_members_gen0_insert_trigger();

CREATE OR REPLACE TRIGGER osm_route_members_gen0_after_update
AFTER UPDATE ON osm_route_members
FOR EACH ROW
EXECUTE FUNCTION osm_route_members_gen0_update_trigger();

CREATE OR REPLACE TRIGGER osm_route_members_gen0_after_delete
AFTER DELETE ON osm_route_members
FOR EACH ROW
EXECUTE FUNCTION osm_route_members_gen0_delete_trigger();

create index osm_shops_type on osm_shops(type);

create index osm_feature_lines_type on osm_feature_lines(type);

CREATE INDEX CONCURRENTLY osm_features_peak_named_geom_gist
ON osm_features
USING GIST (geometry)
WHERE type = 'peak'
  AND name <> '';
