use crate::{
    app::server::app_state::AppState,
    render::{LANDCOVER_OVERLAY_TYPES, LANDCOVER_PAINTS, LANDCOVER_TYPE_SQL, golf_parts_sql},
};
use axum::{
    body::Body,
    extract::{Query, State},
    http::{Response, StatusCode},
};
use serde::Deserialize;
use std::{sync::LazyLock, time::Duration};
use tokio::sync::Semaphore;

const MAX_SIDE_M: f64 = 100_000.0;

/// Keeps the coordinate rounding below a tenth of the grid within 7 decimals.
const MIN_TOLERANCE_M: f64 = 1.0;

const STATEMENT_TIMEOUT: &str = "30s";

/// For a query slot and a connection; with the statement timeout it stays within nginx's 90 s.
const MAX_WAIT: Duration = Duration::from_secs(30);

/// Each query holds a render pool connection for up to the statement timeout.
// TODO derive from --pool-max-size if a deployment ever runs with a small pool.
static SLOTS: Semaphore = Semaphore::const_new(2);

/// Caps the vertex count: a box this many tolerances across is already far finer than
/// any printable model.
const MAX_SIDE_IN_TOLERANCES: f64 = 10_000.0;

const METRES_PER_DEGREE: f64 = 111_320.0;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LandcoverQuery {
    /// `minLon,minLat,maxLon,maxLat` in WGS84.
    bbox: String,
    /// Grid the vertices are snapped to, in ground metres.
    tolerance: f64,
    /// Polygon parts and holes smaller than this, in ground square metres, are dropped.
    min_area: Option<f64>,
}

/// The types the map paints; the rest are boundaries or not drawn at all.
static PAINTED_TYPES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    LANDCOVER_PAINTS
        .iter()
        .filter(|(_, paints)| !paints.is_empty())
        .map(|(typ, _)| *typ)
        .collect()
});

/// The coarsest land table whose simplification (in Web Mercator metres) stays within the grid.
fn land_table(tolerance: f64) -> &'static str {
    match tolerance {
        150.0.. => "land_z5_7",
        30.0.. => "land_z8_10",
        4.0.. => "land_z11_13",
        _ => "land_z14_plus",
    }
}

/// Ordered as the map paints: sea, landcovers, water areas. Everything is snapped to one
/// grid, which keeps a shared edge shared where per-polygon simplification would open seams;
/// that is also why landcovers and water areas come from the full-detail tables.
// TODO the whole FeatureCollection is built as one text value; stream it if a box near the
// limits ever gets dense enough to matter.
fn sql(land_table: &str) -> String {
    let golf = golf_parts_sql();

    format!(
        "
        WITH
            box AS (
                SELECT ST_Transform(ST_MakeEnvelope($1, $2, $3, $4, 4326), 3857) AS g
            ),
            src AS (
                SELECT
                    0 AS layer,
                    'sea' AS type,
                    false AS overlay,
                    false AS tmp,
                    0::float8 AS area,
                    0::bigint AS osm_id,
                    ST_Difference(
                        box.g,
                        COALESCE(
                            (
                                SELECT ST_Union(ST_Intersection(land.geometry, box.g))
                                FROM {land_table} land
                                WHERE land.geometry && box.g
                            ),
                            'SRID=3857;POLYGON EMPTY'::geometry
                        )
                    ) AS geometry
                FROM box

                UNION ALL

                SELECT * FROM (
                    SELECT
                        1,
                        {LANDCOVER_TYPE_SQL} AS type,
                        -- the raw type, as the map orders by: a bog is still a wetland here
                        type IN ({LANDCOVER_OVERLAY_TYPES}),
                        false,
                        area,
                        osm_id,
                        ST_ClipByBox2D(geometry, box.g)
                    FROM osm_landcovers, box
                    WHERE geometry && box.g AND area >= $9::float8
                ) landcover
                WHERE type = ANY($8)

                UNION ALL

                SELECT
                    2,
                    type,
                    false,
                    COALESCE(intermittent OR seasonal, false),
                    area,
                    osm_id,
                    ST_ClipByBox2D(geometry, box.g)
                FROM osm_waterareas, box
                WHERE geometry && box.g AND area >= $9::float8
            ),
            ordered AS (
                SELECT
                    row_number() OVER (
                        ORDER BY layer, overlay, area DESC, osm_id, type IN ({golf}), type
                    ) AS ord,
                    layer,
                    type,
                    tmp,
                    geometry
                FROM src
            ),
            parts AS (
                SELECT
                    ord,
                    layer,
                    type,
                    tmp,
                    (ST_Dump(ST_SnapToGrid(geometry, $5::float8))).geom AS g
                FROM ordered
            ),
            kept AS (
                SELECT
                    ord,
                    layer,
                    type,
                    tmp,
                    CASE WHEN $6::float8 > 0 THEN
                        ST_MakePolygon(
                            ST_ExteriorRing(g),
                            ARRAY(
                                SELECT ST_InteriorRingN(g, n)
                                FROM generate_series(1, ST_NumInteriorRings(g)) n
                                WHERE ST_Area(ST_MakePolygon(ST_InteriorRingN(g, n))) >= $6
                            )
                        )
                    ELSE g END AS g
                FROM parts
                WHERE
                    ST_GeometryType(g) = 'ST_Polygon' AND
                    ($6 = 0 OR ST_Area(ST_MakePolygon(ST_ExteriorRing(g))) >= $6)
            )
        SELECT json_build_object(
            'type', 'FeatureCollection',
            'features', COALESCE(json_agg(feature ORDER BY ord), '[]')
        )::text
        FROM (
            SELECT
                ord,
                json_build_object(
                    'type', 'Feature',
                    'properties', json_strip_nulls(json_build_object(
                        'layer', (ARRAY['sea', 'landcover', 'water'])[layer + 1],
                        'type', type,
                        'intermittent', CASE WHEN tmp THEN true END
                    )),
                    'geometry', ST_AsGeoJSON(ST_Transform(ST_Collect(g), 4326), $7::int)::json
                ) AS feature
            FROM kept
            GROUP BY ord, layer, type, tmp
        ) features
        "
    )
}

fn text_response(status: StatusCode, message: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(message))
        .expect("body should be built")
}

fn bad_request(message: &'static str) -> Response<Body> {
    text_response(StatusCode::BAD_REQUEST, message)
}

pub async fn get(
    State(state): State<AppState>,
    Query(LandcoverQuery {
        bbox,
        tolerance,
        min_area,
    }): Query<LandcoverQuery>,
) -> Response<Body> {
    let Ok(bbox) = bbox
        .split(',')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
    else {
        return bad_request("bbox should be minLon,minLat,maxLon,maxLat");
    };

    let &[min_lon, min_lat, max_lon, max_lat] = bbox.as_slice() else {
        return bad_request("bbox should be minLon,minLat,maxLon,maxLat");
    };

    if !(-180.0..=180.0).contains(&min_lon)
        || !(-180.0..=180.0).contains(&max_lon)
        || !(-85.0..=85.0).contains(&min_lat)
        || !(-85.0..=85.0).contains(&max_lat)
        || min_lon >= max_lon
        || min_lat >= max_lat
    {
        return bad_request("bbox out of range");
    }

    let min_area = min_area.unwrap_or(0.0);

    if !tolerance.is_finite() || tolerance <= 0.0 || !min_area.is_finite() || min_area < 0.0 {
        return bad_request("tolerance should be positive and minArea non-negative");
    }

    if tolerance < MIN_TOLERANCE_M {
        return bad_request("tolerance should be at least 1 m");
    }

    let cos_lat = f64::midpoint(min_lat, max_lat).to_radians().cos();

    let side = ((max_lon - min_lon) * cos_lat).max(max_lat - min_lat) * METRES_PER_DEGREE;

    if side > MAX_SIDE_M {
        return bad_request("bbox too large");
    }

    if side / tolerance > MAX_SIDE_IN_TOLERANCES {
        return bad_request("tolerance too small for the bbox");
    }

    // Web Mercator stretches ground lengths by 1 / cos(latitude).
    let scale = 1.0 / cos_lat;

    // Rounded to a tenth of the tolerance, so the rounding never undoes the snapping.
    let digits = (10.0 * METRES_PER_DEGREE / tolerance)
        .log10()
        .ceil()
        .clamp(0.0, 7.0) as i32;

    let waited = tokio::time::timeout(MAX_WAIT, async {
        let slot = SLOTS.acquire().await.expect("semaphore is never closed");

        state.db_pool.get().await.map(|client| (slot, client))
    })
    .await;

    let (_slot, mut client) = match waited {
        Ok(Ok(acquired)) => acquired,
        Ok(Err(err)) => {
            eprintln!("landcover: db pool: {err}");

            return text_response(StatusCode::SERVICE_UNAVAILABLE, "database unavailable");
        }
        Err(_) => return text_response(StatusCode::SERVICE_UNAVAILABLE, "server busy"),
    };

    match query(
        &mut client,
        tolerance * scale,
        &[
            &min_lon,
            &min_lat,
            &max_lon,
            &max_lat,
            &(tolerance * scale),
            &(min_area * scale * scale),
            &digits,
            &*PAINTED_TYPES,
            // the `area` columns are already ground m²
            &min_area,
        ],
    )
    .await
    {
        Ok(geojson) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/geo+json")
            .body(Body::from(geojson))
            .expect("body should be built"),
        Err(err) => {
            eprintln!("landcover: query failed: {err}");

            text_response(StatusCode::INTERNAL_SERVER_ERROR, "landcover query failed")
        }
    }
}

/// `SET LOCAL` needs a transaction, and keeps the timeout off the pooled connection.
async fn query(
    client: &mut deadpool_postgres::Object,
    tolerance_3857: f64,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<String, tokio_postgres::Error> {
    let mut cancel = CancelOnDrop(Some(client.cancel_token()));

    let tx = client.transaction().await?;

    tx.batch_execute(&format!(
        "SET LOCAL statement_timeout = '{STATEMENT_TIMEOUT}'"
    ))
    .await?;

    let row = tx
        .query_one(&sql(land_table(tolerance_3857)), params)
        .await?;

    tx.commit().await?;

    cancel.0 = None;

    Ok(row.get(0))
}

/// Postgres runs a query to its end even after the client has gone, keeping a render pool
/// connection busy that the released slot no longer accounts for.
struct CancelOnDrop(Option<tokio_postgres::CancelToken>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(token) = self.0.take() {
            tokio::spawn(async move {
                let _ = token.cancel_query(tokio_postgres::NoTls).await;
            });
        }
    }
}
