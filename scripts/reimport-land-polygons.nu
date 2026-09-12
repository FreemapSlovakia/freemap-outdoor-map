#!/usr/bin/env nu

# Re-import the coastline (land polygons) and rebuild the per-zoom land_z* tables.
#
# Coastlines do NOT come from OSM via imposm -- mapping.yaml has no
# natural=coastline. They come from osmdata.openstreetmap.de's pre-assembled
# land-polygons-complete shapefile, which is rebuilt from the planet daily and is
# already a valid polygon set (the raw OSM coastline ways are not). This script
# fetches it, loads it into land_polygons_raw, and runs sql/land-polygons.sql to
# derive the four tables src/render/layers/sea.rs actually reads.
#
# Volume and timing, measured on fm5 2026-09-12 (831 k polygons, NVMe, 100 MB/s down):
#   download    9 s  -> 948 MB zip
#   extract    11 s  -> ~1.7 GB shapefile
#   import     36 s  -> land_polygons_raw, 1369 MB
#   rebuild   ~40 min -> land_z5_7 200 MB, land_z8_10 373 MB, land_z11_13 964 MB,
#                        land_z14_plus 1672 MB -- plus the same again transiently
#                        (see below), so keep ~7 GB free in the tablespace
# The rebuild is essentially all of the wall clock; the fetch and load are noise.
#
# RENDERING KEEPS SERVING THROUGHOUT. Neither stage touches a live table for
# longer than a rename: land_polygons_raw is build input only, never queried at
# runtime, and sql/land-polygons.sql builds land_z*_new and swaps them in one
# short transaction at the end. No service restart -- the renderer resolves the
# table name per query.
#
# RESUMABLE: the download continues (aria2c -c) and is skipped outright once its
# size matches the server's. The import and the rebuild are not resumable, but
# both are safe to repeat -- a failed rebuild leaves the live tables untouched,
# so re-run with --sql-only. Should only the final swap fail (lock_timeout), the
# built *_new tables survive; --sql-only redoes the whole rebuild, or run just
# the swap block of sql/land-polygons.sql by hand.
#
# On fm5 the database roles use peer authentication, so psql and ogr2ogr must run
# as the unix user that owns the tables. Run as martin and let --db-user hand
# them over; the SQL is piped on stdin so only the shapefile needs to be readable
# by that user (it is -- /home/martin is group freemap with traverse):
#
#   scp scripts/reimport-land-polygons.nu sql/land-polygons.sql fm5:land-polygons-reimport/
#   ssh fm5 '~/.cargo/bin/nu land-polygons-reimport/reimport-land-polygons.nu \
#              --db-user freemap --database freemap --sql land-polygons-reimport/land-polygons.sql'
#
# Locally, where the connection needs no sudo, the flags can be dropped and PG*
# environment variables (or the libpq defaults) apply.

const ZIP_URL = "https://osmdata.openstreetmap.de/download/land-polygons-complete-3857.zip"
const NEEDED_GB = 4     # download + extracted shapefile, with headroom

def main [
    --work: string = "~/land-polygons"  # download + extraction directory
    --db-user: string = ""              # run psql/ogr2ogr as this unix user via sudo (peer auth)
    --database: string = ""             # database name; empty = libpq default
    --sql: string = ""                  # path to land-polygons.sql; empty = next to this script
    --sql-only                          # skip download and import, only rebuild the land_z* tables
    --keep                              # keep the zip and shapefile afterwards
] {
    let work = ($work | path expand)

    let sql_file = if ($sql | is-empty) {
        ($env.FILE_PWD | path dirname | path join "sql" "land-polygons.sql")
    } else {
        ($sql | path expand)
    }

    if not ($sql_file | path exists) {
        error make { msg: $"SQL file not found: ($sql_file) -- pass --sql" }
    }

    if not $sql_only {
        mkdir $work

        # ── Download ──────────────────────────────────────────────────────────
        let avail_gb = (avail-gb $work)

        if $avail_gb < $NEEDED_GB {
            error make { msg: $"only ($avail_gb) GB free on ($work), need ~($NEEDED_GB) GB" }
        }

        let zip = ($work | path join ($ZIP_URL | path basename))
        let want = (remote-size $ZIP_URL)

        if ($zip | path exists) and $want > 0 and (local-size $zip) == $want {
            say $"zip already complete, skipping download: ($zip)"
        } else {
            say $"downloading ($ZIP_URL) -> ($zip)"

            ^aria2c -c -x 16 -s 16 --dir $work --out ($zip | path basename) $ZIP_URL

            let got = (local-size $zip)

            if $want > 0 and $got != $want {
                error make { msg: $"short download: ($got) of ($want) bytes -- re-run to continue" }
            }
        }

        # ── Extract ───────────────────────────────────────────────────────────
        say "extracting"

        ^unzip -o -q $zip -d $work

        let shp = (glob ($work | path join "**" "*.shp"))

        if ($shp | length) != 1 {
            error make { msg: $"expected exactly one .shp under ($work), found ($shp | length): ($shp)" }
        }

        let shp = ($shp | first)

        # ── Import ────────────────────────────────────────────────────────────
        # -overwrite drops land_polygons_raw first, so a failure here loses the
        # build input but leaves the live land_z* tables serving. PG_USE_COPY
        # turns the load into COPY instead of a few hundred thousand INSERTs.
        say $"importing ($shp) -> land_polygons_raw"

        db-run $db_user "ogr2ogr" [
            "-f" "PostgreSQL" $"PG:(pg-conn $database)" $shp
            "-nln" "land_polygons_raw"
            "-lco" "GEOMETRY_NAME=geom"
            "-lco" "FID=osm_id"
            "-lco" "SPATIAL_INDEX=GIST"
            "-t_srs" "EPSG:3857"
            "-nlt" "PROMOTE_TO_MULTI"
            "-overwrite"
            "-progress"
            "--config" "PG_USE_COPY" "YES"
        ]

        psql-do $db_user $database "ANALYZE land_polygons_raw;"
    }

    # ── Rebuild the per-zoom tables and swap them in ──────────────────────────
    say $"rebuilding land_z* from ($sql_file)"

    open --raw $sql_file | psql-run $db_user $database ["-v" "ON_ERROR_STOP=1" "-f" "-"]

    # ── Report ────────────────────────────────────────────────────────────────
    psql-do $db_user $database "
        SELECT
            relname AS table,
            n_live_tup AS rows,
            pg_size_pretty(pg_total_relation_size(relid)) AS size
        FROM pg_stat_user_tables
        WHERE relname LIKE 'land\\_%'
        ORDER BY relname;
    "

    if not $keep and not $sql_only {
        say "removing downloaded zip and shapefile"

        rm -rf $work
    } else if not $sql_only {
        say $"keeping ($work) -- delete it once the map looks right"
    }

    say "done -- no service restart needed"
}

# ── Helpers ───────────────────────────────────────────────────────────────────

def say [msg: string] {
    print $"[(date now | format date '%H:%M:%S')] ($msg)"
}

def pg-conn [database: string]: nothing -> string {
    if ($database | is-empty) { "" } else { $"dbname=($database)" }
}

def local-size [f: string]: nothing -> int {
    if ($f | path exists) { (ls -l $f | get 0.size | into int) } else { 0 }
}

# Content-Length of the final URL in the redirect chain, 0 if the server withheld it.
def remote-size [url: string]: nothing -> int {
    let lens = (
        ^curl -sSIL --retry 3 $url
            | lines
            | where {|l| $l =~ '(?i)^content-length:' }
    )

    if ($lens | is-empty) { 0 } else { $lens | last | split row ":" | get 1 | str trim | into int }
}

# Free space of the filesystem holding `dir`, in whole GB. `dir` may not exist yet,
# so ask about its nearest existing ancestor.
def avail-gb [dir: string]: nothing -> int {
    mut d = ($dir | path expand --no-symlink)

    while not ($d | path exists) {
        $d = ($d | path dirname)
    }

    (^df -B1 --output=avail $d | lines | last | str trim | into int) // (1024 * 1024 * 1024)
}

# Run an external command, optionally as another unix user (peer authentication
# needs the connecting unix user to match the database role). sudo strips PG*, so
# the database is always named explicitly when --database is given.
def db-run [user: string, cmd: string, args: list<string>] {
    # $in must be forwarded explicitly: a custom command's pipeline input does NOT
    # reach an external command in its body on its own, and a psql reading `-f -`
    # off an unfed stdin opens an interactive session instead of running the file.
    let input = $in

    if ($user | is-empty) {
        $input | ^$cmd ...$args
    } else {
        $input | ^sudo -n -u $user $cmd ...$args
    }
}

def psql-run [user: string, database: string, args: list<string>] {
    let input = $in
    let db = if ($database | is-empty) { [] } else { ["-d" $database] }

    $input | db-run $user "psql" ($db | append $args)
}

def psql-do [user: string, database: string, sql: string] {
    psql-run $user $database ["-v" "ON_ERROR_STOP=1" "-c" $sql]
}
