\getenv db_name PGDATABASE
\getenv db_password PGPASSWORD
\getenv db_user PGUSER

GRANT CREATE ON DATABASE :db_name TO :db_user;

ALTER USER :db_user WITH PASSWORD :db_password;

GRANT ALL ON SCHEMA public TO :db_name;

CREATE EXTENSION IF NOT EXISTS postgis;

CREATE EXTENSION IF NOT EXISTS postgis_topology;

CREATE EXTENSION IF NOT EXISTS intarray;

CREATE EXTENSION IF NOT EXISTS hstore;

-- see https://wiki.postgresql.org/wiki/First/last_(aggregate)
-- Create a function that always returns the first non-NULL item
CREATE
OR REPLACE FUNCTION public.first_agg (anyelement, anyelement) RETURNS anyelement LANGUAGE SQL IMMUTABLE STRICT AS $$
SELECT $1;
$$;

-- And then wrap an aggregate around it
CREATE AGGREGATE public.FIRST (
        sfunc = public.first_agg,
        basetype = anyelement,
        stype = anyelement
);

-- Create a function that always returns the last non-NULL item
CREATE
OR REPLACE FUNCTION public.last_agg (anyelement, anyelement) RETURNS anyelement LANGUAGE SQL IMMUTABLE STRICT AS $$
SELECT $2;
$$;

-- And then wrap an aggregate around it
CREATE AGGREGATE public.LAST (
        sfunc = public.last_agg,
        basetype = anyelement,
        stype = anyelement
);
