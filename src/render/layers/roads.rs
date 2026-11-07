use crate::render::{
    Feature, FeatureError,
    colors::{self, Color, ContextExt},
    ctx::Ctx,
    draw::path_geom::{path_line_string, path_line_string_with_offset},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;
use std::borrow::Cow;

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let zoom = ctx.zoom;

    let table = match zoom {
        ..=9 => "osm_roads_gen0",
        10..=11 => "osm_roads_gen1",
        12.. => "osm_roads",
    };

    // Only strokes bleed in from outside the tile, and none here is wider than ~7 px.
    // Labels, oneway arrows and access marks are separate layers, each with its own buffer.
    const BUFFER_PX: f64 = 16.0;

    // Only zoom 12 reads it: every arm using it is `zoom > 12 || is_in_route`.
    let select_member = if zoom == 12 {
        ",osm_route_members.member IS NOT NULL AS is_in_route"
    } else {
        ""
    };

    #[cfg_attr(any(), rustfmt::skip)]
    let join_members: Cow<_> = if zoom == 12 {
        format!("
            LEFT JOIN
                osm_route_members
            ON
                osm_route_members.type = 1 AND
                osm_route_members.member = -{table}.osm_id
        ").into()
    } else {
        "".into()
    };

    // A closed way without `area=no` lands in `osm_landcovers` too, and `pier_areas` fills it
    // from there; `area=no` keeps a closed pier out of it, so it is still drawn as a line.
    let select_pier_area = if zoom >= 14 {
        format!(
            ",
            {table}.type = 'pier' AND EXISTS (
                SELECT 1 FROM osm_landcovers
                WHERE osm_landcovers.osm_id = {table}.osm_id AND osm_landcovers.type = 'pier'
            ) AS is_pier_area"
        )
    } else {
        String::new()
    };

    // A detail is drawn along a way only where its run is long enough to show it; a short
    // run is an icon of the POI layer.
    let (select_trail_details, join_trail_runs) = if zoom >= 14 {
        let select = TRAIL_DETAILS
            .map(|key| format!("COALESCE('{key}' = ANY(trail_runs.long_keys), false) AS {key}"))
            .join(",\n");

        #[cfg_attr(any(), rustfmt::skip)]
        let join = format!("
            LEFT JOIN (
                SELECT osm_id, type, array_agg(key) FILTER (WHERE NOT short) AS long_keys
                FROM ({runs}) AS runs
                GROUP BY osm_id, type
            ) AS trail_runs
            ON
                -- A way of several types, such as a platform, is a row of each.
                trail_runs.osm_id = {table}.osm_id AND trail_runs.type = {table}.type
        ", runs = trail_detail_runs_sql(ctx, "$5"));

        (select, join)
    } else {
        (
            TRAIL_DETAILS
                .map(|key| format!("false AS {key}"))
                .join(", "),
            String::new(),
        )
    };

    #[cfg_attr(any(), rustfmt::skip)]
    let query = format!("
        SELECT
            {table}.geometry,
            {table}.type,
            tracktype,
            class,
            service,
            bridge,
            tunnel,
            bicycle,
            foot,
            trail_visibility,
            {select_trail_details}
            {select_member}
            {select_pier_area}
        FROM
            {table}
            {join_members}
            {join_trail_runs}
        WHERE
            {table}.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            z_order,
            CASE WHEN {table}.type = 'rail' AND service IN ('', 'main') THEN 2 ELSE 1 END,
            {table}.osm_id
    ");

    client
        .query(&query, &ctx.bbox_query_params(Some(BUFFER_PX)).as_params())
        .await
}

/// On the side the bicycle and ski route marking takes, as a path with a rope only ever
/// carries hiking marking, which keeps the other side.
const SAFETY_ROPE_OFFSET: f64 = -3.5;

/// A `rail` of this `service` is drawn as a main line; any other as light rail.
fn is_main_line(service: &str) -> bool {
    matches!(service, "" | "main")
}

/// How [`render`] draws a road, one per arm; the legend lists by it too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadStyle {
    Pier,
    Rail,
    LightRail,
    MinorRail,
    RailConstruction,
    DisusedRail,
    RailLow,
    MotorwayLow,
    PrimaryLow,
    SecondaryLow,
    TertiaryLow,
    Motorway,
    MotorwayLink,
    Primary,
    Secondary,
    Construction,
    Tertiary,
    ResidentialLow,
    Residential,
    WaterSlide,
    ParkingAisle,
    Raceway,
    Piste,
    Footway,
    Platform,
    Steps,
    Service,
    PathBikeFoot,
    Cycleway,
    Path,
    Bridleway,
    ViaFerrata,
    Track,
    Ladder,
}

impl RoadStyle {
    /// Whether [`render`] draws bridge and tunnel casings on it.
    pub const fn has_casings(self) -> bool {
        !matches!(
            self,
            Self::MotorwayLow
                | Self::PrimaryLow
                | Self::SecondaryLow
                | Self::TertiaryLow
                | Self::Construction
                | Self::Steps
        )
    }
}

/// The `highway` types a `ladder=*` way of which is drawn as a ladder.
pub const LADDER_HIGHWAYS: [&str; 4] = ["path", "footway", "steps", "via_ferrata"];

const TRAIL_DETAILS: [&str; 3] = ["rungs", "safety_rope", "ladder"];

/// The `highway` types a trail detail is drawn on.
const TRAIL_HIGHWAYS: [&str; 8] = [
    "footway",
    "pedestrian",
    "steps",
    "path",
    "cycleway",
    "bridleway",
    "via_ferrata",
    "ladder",
];

/// How many pixels a run of touching ways carrying a trail detail has to span for the
/// detail to be drawn along it. A shorter run is drawn as an icon instead.
const TRAIL_DETAIL_MIN_PX: f64 = 12.0;

/// The SQL conditions on `osm_roads` for a way to carry each of [`TRAIL_DETAILS`].
fn trail_detail_conds() -> [String; 3] {
    let trail = format!(
        "class = 'highway' AND type IN ({})",
        TRAIL_HIGHWAYS.map(|typ| format!("'{typ}'")).join(", ")
    );

    let ladder = format!(
        "(type = 'ladder' OR type IN ({}) AND COALESCE(tags->'ladder', 'no') <> 'no')",
        LADDER_HIGHWAYS.map(|typ| format!("'{typ}'")).join(", ")
    );

    [
        format!("{trail} AND COALESCE(tags->'rungs', 'no') <> 'no' AND NOT {ladder}"),
        format!("{trail} AND COALESCE(tags->'safety_rope', 'no') <> 'no'"),
        ladder,
    ]
}

/// SQL for the `osm_roads` ways near the tile that carry a trail detail, a row per way and
/// detail: `osm_id`, `type`, `geometry`, the detail as `key`, the `run` of touching ways
/// carrying it, and whether that run is `short` of [`TRAIL_DETAIL_MIN_PX`]. Ways are fetched
/// that length beyond `buffer`, the caller's reach past the tile, so a run reaching into it is
/// fetched whole or already long, and short on every tile or on none.
pub fn trail_detail_runs_sql(ctx: &Ctx, buffer: &str) -> String {
    let min_length = ctx.meters_per_pixel() * TRAIL_DETAIL_MIN_PX;

    let details = TRAIL_DETAILS
        .iter()
        .zip(trail_detail_conds())
        .map(|(key, cond)| format!("('{key}', {cond})"))
        .collect::<Vec<_>>()
        .join(", ");

    // The first condition is the partial index's, in `sql/additional.sql`.
    let parts = format!(
        "SELECT key, osm_id, type, geometry
        FROM osm_roads CROSS JOIN LATERAL (VALUES {details}) AS details (key, carries)
        WHERE
            (type = 'ladder' OR tags ?| ARRAY['ladder', 'rungs', 'safety_rope']) AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), {buffer}::float8 + {min_length}) AND
            carries"
    );

    format!(
        "SELECT
            key, osm_id, type, geometry, run,
            SUM(ST_Length(geometry)) OVER (PARTITION BY key, run) < {min_length} AS short
        FROM (
            SELECT *, ST_ClusterDBSCAN(geometry, 0.01, 1) OVER (PARTITION BY key) AS run
            FROM ({parts}) AS details
        ) AS clustered"
    )
}

/// What [`render`] draws `row` as at `zoom`, or `None` where it draws nothing.
pub fn style(zoom: u8, row: &Feature) -> Result<Option<RoadStyle>, FeatureError> {
    let typ = row.get_string("type")?;
    let class = row.get_string("class")?;
    let service = row.get_string("service")?;
    let bicycle = row.get_string("bicycle")?;
    let foot = row.get_string("foot")?;
    let tracktype = row.get_string("tracktype")?;

    // The query has the column at zoom 12 alone.
    let routed = zoom > 12 || zoom == 12 && row.get_bool("is_in_route")?;

    Ok(Some(match (zoom, class, typ) {
        (14.., "highway", _) if row.get_bool("ladder")? => RoadStyle::Ladder,
        (14.., _, "pier") if !row.get_bool("is_pier_area")? => RoadStyle::Pier,
        (12.., "railway", "rail") if is_main_line(service) => RoadStyle::Rail,
        (13.., "railway", _)
            if ["light_rail", "tram"].contains(&typ) || typ == "rail" && !is_main_line(service) =>
        {
            RoadStyle::LightRail
        }
        (13.., "railway", "miniature" | "monorail" | "funicular" | "narrow_gauge" | "subway") => {
            RoadStyle::MinorRail
        }
        (13.., "railway", "construction") => RoadStyle::RailConstruction,
        (14.., "railway", "disused" | "preserved") => RoadStyle::DisusedRail,
        (8..=11, "railway", "rail") if is_main_line(service) => RoadStyle::RailLow,
        (8..=11, "highway", "motorway" | "trunk" | "motorway_link" | "trunk_link") => {
            RoadStyle::MotorwayLow
        }
        (8..=11, "highway", "primary" | "primary_link") => RoadStyle::PrimaryLow,
        (8..=11, "highway", "secondary" | "secondary_link") => RoadStyle::SecondaryLow,
        (8..=11, "highway", "tertiary" | "tertiary_link") => RoadStyle::TertiaryLow,
        (12.., "highway", "motorway" | "trunk") => RoadStyle::Motorway,
        (12.., "highway", "motorway_link" | "trunk_link") => RoadStyle::MotorwayLink,
        (12.., "highway", "primary") => RoadStyle::Primary,
        (12.., "highway", "primary_link" | "secondary") => RoadStyle::Secondary,
        (12.., "highway", "construction") => RoadStyle::Construction,
        (12.., "highway", "secondary_link" | "tertiary" | "tertiary_link") => RoadStyle::Tertiary,
        (12..=13, "highway", "living_street" | "residential" | "unclassified" | "road") => {
            RoadStyle::ResidentialLow
        }
        (14.., "highway", "living_street" | "residential" | "unclassified" | "road") => {
            RoadStyle::Residential
        }
        (14.., "attraction", "water_slide") => RoadStyle::WaterSlide,
        (14.., "highway", "service") if service == "parking_aisle" => RoadStyle::ParkingAisle,
        (14.., _, "raceway") | (14.., "leisure", "track") => RoadStyle::Raceway,
        (14.., "highway", "piste") => RoadStyle::Piste,
        (14.., "highway", "footway" | "pedestrian") => RoadStyle::Footway,
        (14.., "highway" | "railway" | "public_transport", "platform") => RoadStyle::Platform,
        (14.., "highway", "steps") => RoadStyle::Steps,
        (12.., "highway", _)
            if typ == "service" && service != "parking_aisle"
                || ["escape", "corridor", "bus_guideway"].contains(&typ) =>
        {
            RoadStyle::Service
        }
        (12.., "highway", "path") if bicycle == "designated" && foot == "designated" && routed => {
            RoadStyle::PathBikeFoot
        }
        (12.., "highway", _)
            if (typ == "cycleway"
                || typ == "path" && bicycle == "designated" && foot != "designated")
                && routed =>
        {
            RoadStyle::Cycleway
        }
        (12.., "highway", "path" | "ladder") if routed => RoadStyle::Path,
        (12.., "highway", "bridleway") if routed => RoadStyle::Bridleway,
        (12.., "highway", "via_ferrata") if routed => RoadStyle::ViaFerrata,
        (12.., "highway", "track") if routed || tracktype == "grade1" => RoadStyle::Track,
        _ => return Ok(None),
    }))
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("roads::render");

    let zoom = ctx.zoom;

    let apply_highway_defaults = |width: f64| {
        context.set_dash(&[], 0.0);
        context.set_source_color(colors::TRACK);
        context.set_line_join(cairo::LineJoin::Round);
        context.set_line_width(width);
    };

    let apply_glow_defaults_a = |width: f64, alpha: f64| {
        context.set_source_color_a(colors::GLOW, alpha);
        context.set_dash(&[], 0.0);
        context.set_line_join(cairo::LineJoin::Round);
        context.set_line_width(width);
    };

    let apply_glow_defaults = |width: f64| {
        apply_glow_defaults_a(width, 1.0);
    };

    let highway_width_coef = || 1.5f64.powf(8.6f64.max(zoom as f64) - 8.0);

    let ke = || match zoom {
        12 => 0.66,
        13 => 0.75,
        14.. => 1.00,
        _ => 0.00,
    };

    context.save()?;

    let rows: Vec<_> = rows
        .iter()
        .map(|row| {
            Ok((
                row,
                row.get_line_string()?.project_to_tile(&ctx.tile_projector),
            ))
        })
        .collect::<Result<Vec<_>, FeatureError>>()?;

    for (row, geom) in &rows {
        let typ = row.get_string("type")?;
        let class = row.get_string("class")?;
        let bicycle = row.get_string("bicycle")?;
        let tracktype = row.get_string("tracktype")?;
        let service = row.get_string("service")?;

        let draw = || -> cairo::Result<()> {
            path_line_string(context, geom);

            context.stroke()?;

            Ok(())
        };

        let is_in_route = if zoom == 12 {
            row.get_bool("is_in_route")?
        } else {
            false
        };

        let trail_visibility = 0.666f64.powf(row.get_i32("trail_visibility")? as f64);

        // Between the rails a glow would read as a middle line.
        if row.get_bool("ladder")? {
            continue;
        }

        match (zoom, class, typ) {
            (..=11, _, _) => (),
            (14.., "highway", "footway" | "pedestrian" | "steps")
            | (14.., "railway", "platform") => {
                apply_glow_defaults(1.0);
                draw()?;
            }
            (14.., "highway", "via_ferrata") => {
                apply_glow_defaults(3.0);
                context.set_source_rgb(0.0, 0.0, 0.0);
                context.set_dash(&[0.0, 4.0, 4.0, 0.0], 0.0);
                draw()?;

                apply_glow_defaults(1.0);
                draw()?;
            }
            (12.., "highway", "path" | "ladder")
                if (typ == "ladder" || bicycle != "designated") && (zoom > 12 || is_in_route) =>
            {
                apply_glow_defaults_a(1.0, trail_visibility);
                draw()?;
            }
            (12.., "highway", _)
                if typ == "track" && (zoom > 12 || is_in_route || tracktype == "grade1")
                    || typ == "service" && service != "parking_aisle"
                    || ["escape", "corridor", "bus_guideway"].contains(&typ) =>
            {
                apply_glow_defaults_a(ke() * 1.2, trail_visibility);
                draw()?;
            }
            (14.., _, "raceway") | (14.., "leisure", "track") => {
                apply_glow_defaults(1.2);
                draw()?;
            }
            (13.., "highway", "bridleway") => {
                apply_glow_defaults(1.2);
                context.set_source_color_a(colors::BRIDLEWAY2, trail_visibility);
                draw()?;
            }
            (_, "highway", "motorway" | "trunk") => {
                apply_highway_defaults(4.0);
                draw()?;
            }
            (_, "highway", "primary" | "motorway_link" | "trunk_link") => {
                apply_highway_defaults(3.666);
                draw()?;
            }
            (_, "highway", "primary_link" | "secondary" | "construction")
            | (13.., "railway", "construction") => {
                apply_highway_defaults(3.333);
                draw()?;
            }
            (_, "highway", "secondary_link" | "tertiary" | "tertiary_link") => {
                apply_highway_defaults(3.0);
                draw()?;
            }
            (14.., "highway", "living_street" | "residential" | "unclassified" | "road") => {
                apply_highway_defaults(2.5);
                draw()?;
            }
            (14.., "highway", "piste") => {
                apply_highway_defaults(2.2);
                context.set_dash(&[6.0, 2.0], 0.0);
                context.set_source_color(colors::PISTE2);
                draw()?;
            }
            _ => (),
        }
    }

    for (row, geom) in &rows {
        let tracktype = row.get_string("tracktype")?;

        let draw = || -> cairo::Result<()> {
            path_line_string(context, geom);

            context.stroke()?;

            Ok(())
        };

        let bridge = row.get_i16("bridge")?;
        let tunnel = row.get_i16("tunnel")?;

        // Two offset lines rather than a wide stroke with its middle cleared: Cairo's PDF
        // backend rasterizes the `Clear` operator.
        let draw_casing = |offset: f64, width: f64| -> cairo::Result<()> {
            context.set_line_cap(cairo::LineCap::Butt);
            context.set_line_width(width);
            path_line_string_with_offset(context, geom, offset);
            path_line_string_with_offset(context, geom, -offset);
            context.stroke()
        };

        let draw_bridges_tunnels = |width: f64| -> cairo::Result<()> {
            if bridge > 0 {
                context.save()?;
                context.set_dash(&[], 0.0);
                context.set_source_rgb(0.0, 0.0, 0.0);
                draw_casing(width / 2.0 + 0.5, 1.0)?;
                context.restore()?;
            }

            if tunnel > 0 {
                context.set_dash(&[], 0.0);
                context.set_line_width(width + 1.0);

                context.set_source_rgba(0.8, 0.8, 0.8, 0.8);
                draw()?;

                context.save()?;
                context.set_dash(&[3.0, 3.0], 0.0);
                context.set_source_rgba(0.0, 0.0, 0.0, 0.5);
                draw_casing(width / 2.0 + 0.7, 0.6)?;
                context.restore()?;
            }

            Ok(())
        };

        let draw_rail = |color: Color,
                         weight: f64,
                         sleeper_weight: f64,
                         spacing: f64,
                         glow_width: f64|
         -> cairo::Result<()> {
            context.set_line_join(cairo::LineJoin::Round);

            let gw = glow_width.mul_add(2.0, weight);

            let sgw = glow_width.mul_add(2.0, sleeper_weight);

            context.set_source_color(colors::RAIL_GLOW);
            context.set_dash(&[], 0.0);
            context.set_line_width(gw);
            path_line_string(context, geom);
            context.stroke()?;

            context.set_dash(&[0.0, (spacing - gw) / 2.0, gw, (spacing - gw) / 2.0], 0.0);
            context.set_line_width(sgw);
            path_line_string(context, geom);
            context.stroke()?;

            context.set_source_color(color);
            context.set_dash(&[], 0.0);
            context.set_line_width(weight);
            path_line_string(context, geom);
            context.stroke()?;

            context.set_dash(
                &[
                    0.0,
                    (spacing - weight) / 2.0,
                    weight,
                    (spacing - weight) / 2.0,
                ],
                0.0,
            );
            context.set_line_width(sleeper_weight);
            path_line_string(context, geom);
            context.stroke()?;

            draw_bridges_tunnels(sleeper_weight + glow_width)?;

            Ok(())
        };

        let trail_visibility = 0.666f64.powf(row.get_i32("trail_visibility")? as f64);

        let road_style = style(zoom, row)?;

        match road_style {
            Some(RoadStyle::Pier) => {
                apply_highway_defaults(2.0);
                context.set_source_color(colors::PIER);
                draw()?;
            }
            Some(RoadStyle::Rail) => {
                draw_rail(colors::RAIL, 1.5, 5.0, 9.5, 1.0)?;
            }
            Some(RoadStyle::LightRail) => {
                draw_rail(colors::TRAM, 1.0, 4.5, 9.5, 1.0)?;
            }
            Some(RoadStyle::MinorRail) => {
                draw_rail(colors::TRAM, 1.0, 4.5, 7.5, 1.0)?;
            }
            Some(RoadStyle::RailConstruction) => {
                draw_rail(colors::RAILWAY_DISUSED, 1.0, 4.5, 7.5, 1.0)?;

                apply_highway_defaults(1.5 + 1.0 / 3.0);
                context.set_source_color(colors::CONSTRUCTION_ROAD_1);
                context.set_dash(&[5.0, 5.0], 0.0);
                draw()?;

                context.set_source_color(colors::CONSTRUCTION_ROAD_2);
                context.set_dash(&[5.0, 5.0], 5.0);
                draw()?;
            }
            Some(RoadStyle::DisusedRail) => {
                draw_rail(colors::RAILWAY_DISUSED, 1.0, 4.5, 7.5, 1.0)?;
            }
            Some(RoadStyle::RailLow) => {
                let koef = 0.8 * 1.15f64.powf((zoom - 8) as f64);

                draw_rail(
                    colors::RAIL,
                    koef,
                    10.0 / 3.0 * koef,
                    9.5 / 1.5 * koef,
                    0.5 * koef,
                )?;
            }
            Some(RoadStyle::MotorwayLow) => {
                apply_highway_defaults(0.8 * highway_width_coef());
                draw()?;
            }
            Some(RoadStyle::PrimaryLow) => {
                apply_highway_defaults(0.7 * highway_width_coef());
                draw()?;
            }
            Some(RoadStyle::SecondaryLow) => {
                apply_highway_defaults(0.6 * highway_width_coef());
                draw()?;
            }
            Some(RoadStyle::TertiaryLow) => {
                apply_highway_defaults(0.5 * highway_width_coef());
                draw()?;
            }
            Some(RoadStyle::Motorway) => {
                apply_highway_defaults(2.5);
                context.set_source_color(colors::SUPERROAD);
                draw()?;

                draw_bridges_tunnels(2.5 + 1.0)?;
            }
            Some(RoadStyle::MotorwayLink) => {
                apply_highway_defaults(1.5 + 2.0 / 3.0);
                context.set_source_color(colors::SUPERROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 2.0 / 3.0 + 1.0)?;
            }
            Some(RoadStyle::Primary) => {
                apply_highway_defaults(1.5 + 2.0 / 3.0);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 2.0 / 3.0 + 1.0)?;
            }
            Some(RoadStyle::Secondary) => {
                apply_highway_defaults(1.5 + 1.0 / 3.0);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 1.0 / 3.0 + 1.0)?;
            }
            Some(RoadStyle::Construction) => {
                apply_highway_defaults(1.5 + 1.0 / 3.0);
                context.set_source_color(colors::CONSTRUCTION_ROAD_1);
                context.set_dash(&[5.0, 5.0], 0.0);
                draw()?;

                context.set_source_color(colors::CONSTRUCTION_ROAD_2);
                context.set_dash(&[5.0, 5.0], 5.0);
                draw()?;
            }
            Some(RoadStyle::Tertiary) => {
                apply_highway_defaults(1.5);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 1.0 / 3.0 + 1.0)?;
            }
            Some(RoadStyle::ResidentialLow | RoadStyle::ParkingAisle) => {
                apply_highway_defaults(1.0);
                draw()?;

                draw_bridges_tunnels(1.0 + 1.0)?;
            }
            Some(RoadStyle::Residential) => {
                apply_highway_defaults(1.0);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.0 + 1.0)?;
            }
            Some(RoadStyle::WaterSlide) => {
                apply_highway_defaults(1.5);
                context.set_source_color(colors::WATER_SLIDE);
                draw()?;

                draw_bridges_tunnels(1.5 + 1.0)?;
            }
            Some(RoadStyle::Raceway) => {
                apply_highway_defaults(1.2);
                context.set_dash(&[9.5, 1.5], 0.0);
                draw()?;

                draw_bridges_tunnels(1.2 + 1.0)?;
            }
            Some(RoadStyle::Piste) => {
                apply_highway_defaults(1.2);
                context.set_source_color(colors::PISTE);
                draw()?;

                draw_bridges_tunnels(1.2 + 1.0)?;
            }
            Some(RoadStyle::Footway | RoadStyle::Platform) => {
                apply_highway_defaults(1.0);
                context.set_dash(&[4.0, 2.0], 0.0);
                draw()?;

                draw_bridges_tunnels(1.0 + 1.0)?;
            }
            Some(RoadStyle::Steps) => {
                apply_highway_defaults(2.5);
                context.set_dash(&[1.0, 2.0], 2.0);
                draw()?;
            }
            Some(RoadStyle::Service) => {
                let width = ke() * 1.2;

                apply_highway_defaults(width);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            Some(RoadStyle::PathBikeFoot) => {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[4.0, 2.0], 0.0);
                context.set_source_color_a(colors::CYCLEWAY, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            Some(RoadStyle::Cycleway) => {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[6.0, 3.0], 0.0);
                context.set_source_color_a(colors::CYCLEWAY, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            Some(RoadStyle::Path) => {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[3.0, 3.0], 0.0);
                context.set_source_color_a(colors::TRACK, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            Some(RoadStyle::Bridleway) => {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[6.0, 3.0], 0.0);
                context.set_source_color_a(colors::BRIDLEWAY, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            Some(RoadStyle::ViaFerrata) => {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[4.0, 4.0], 0.0);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            Some(RoadStyle::Track) => {
                let width = ke() * 1.2;

                apply_highway_defaults(width);

                context.set_dash(
                    match tracktype {
                        "grade1" => &[],
                        "grade2" => &[8.0, 2.0],
                        "grade3" => &[6.0, 4.0],
                        "grade4" => &[4.0, 6.0],
                        "grade5" => &[2.0, 8.0],
                        _ => &[3.0, 7.0, 7.0, 3.0],
                    },
                    0.0,
                );

                context.set_source_color_a(colors::TRACK, trail_visibility);

                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }

            Some(RoadStyle::Ladder) => {
                apply_highway_defaults(0.7);
                context.set_source_color_a(colors::TRACK, trail_visibility);
                draw_casing(1.5, 0.7)?;

                context.set_line_width(3.7);
                context.set_dash(&[0.8, 3.2], 0.0);
                draw()?;

                draw_bridges_tunnels(4.0)?;
            }
            None => (),
        }

        // Staggered, so that it does not read as the rungs of a ladder.
        if row.get_bool("rungs")? {
            context.set_source_color_a(colors::TRACK, trail_visibility);
            context.set_line_cap(cairo::LineCap::Butt);
            context.set_line_width(1.5);

            for (offset, phase) in [(1.25, 0.0), (-1.25, 3.0)] {
                context.set_dash(&[1.0, 5.0], phase);
                path_line_string_with_offset(context, geom, offset);
                context.stroke()?;
            }
        }

        if row.get_bool("safety_rope")? {
            context.set_source_rgba(0.0, 0.0, 0.0, trail_visibility);
            context.set_line_join(cairo::LineJoin::Round);

            context.set_dash(&[], 0.0);
            context.set_line_width(0.6);
            path_line_string_with_offset(context, geom, SAFETY_ROPE_OFFSET);
            context.stroke_preserve()?;

            // Anchors.
            context.set_line_cap(cairo::LineCap::Round);
            context.set_dash(&[0.0, 8.0], 4.0);
            context.set_line_width(1.6);
            context.stroke()?;
            context.set_line_cap(cairo::LineCap::Butt);
        }
    }

    context.restore()?;

    Ok(())
}
