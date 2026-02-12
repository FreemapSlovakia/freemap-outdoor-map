use std::borrow::Cow;

use crate::render::{
    FeatureError,
    colors::{self, Color, ContextExt},
    ctx::Ctx,
    draw::{markers_on_path::draw_markers_on_path, path_geom::path_line_string},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("roads::render");

    let context = ctx.context;

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("roads", || {
        let table = match zoom {
            ..=9 => "osm_roads_gen0",
            10..=11 => "osm_roads_gen1",
            12.. => "osm_roads",
        };

        // TODO for zoom < 12 we select too much

        let select_member = if zoom <= 12 {
            ",osm_route_members.member IS NOT NULL AS is_in_route"
        } else {
            ""
        };

        #[cfg_attr(any(), rustfmt::skip)]
        let join_members: Cow<_> = if zoom <= 12 {
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
                oneway,
                bicycle,
                foot,
                trail_visibility
                {select_member}
            FROM
                {table}
                {join_members}
            WHERE
                {table}.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                z_order,
                CASE WHEN {table}.type = 'rail' AND service IN ('', 'main') THEN 2 ELSE 1 END,
                {table}.osm_id
        ");

        client.query(&query, &ctx.bbox_query_params(Some(128.0)).as_params())
    })?;

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

    // TODO lazy
    let arrow = svg_repo.get("highway-arrow")?;

    let rect = arrow.extents().expect("surface extents");

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

        let is_in_route = if zoom <= 12 {
            row.get_bool("is_in_route")?
        } else {
            false
        };

        let trail_visibility = 0.666f64.powf(row.get_i32("trail_visibility")? as f64);

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
            (12.., "highway", "path") if bicycle != "designated" && (zoom > 12 || is_in_route) => {
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
            (_, "highway", "primary_link" | "secondary") | (_, _, "construction") => {
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
        let typ = row.get_string("type")?;
        let class = row.get_string("class")?;
        let service = row.get_string("service")?;
        let bicycle = row.get_string("bicycle")?;
        let foot = row.get_string("foot")?;
        let tracktype = row.get_string("tracktype")?;

        let draw = || -> cairo::Result<()> {
            path_line_string(context, geom);

            context.stroke()?;

            Ok(())
        };

        let bridge = row.get_i16("bridge")?;
        let tunnel = row.get_i16("tunnel")?;

        let draw_bridges_tunnels = |width: f64| -> cairo::Result<()> {
            if bridge > 0 {
                context.save()?;
                context.set_dash(&[], 0.0);
                context.set_source_rgb(0.0, 0.0, 0.0);

                context.push_group();

                context.set_line_cap(cairo::LineCap::Butt);
                context.set_line_width(width + 2.0);
                draw()?;
                context.stroke()?;

                context.set_line_cap(cairo::LineCap::Square);
                context.set_operator(cairo::Operator::Clear);
                context.set_line_width(width);
                draw()?;
                context.stroke()?;

                context.pop_group_to_source()?;
                context.paint()?;

                context.restore()?;
            }

            if tunnel > 0 {
                context.set_dash(&[], 0.0);
                context.set_line_width(width + 1.0);

                context.set_source_rgba(0.8, 0.8, 0.8, 0.8);
                draw()?;
                context.stroke()?;

                context.save()?;
                context.set_dash(&[3.0, 3.0], 0.0);
                context.set_source_rgba(0.0, 0.0, 0.0, 0.5);

                context.push_group();

                context.set_line_cap(cairo::LineCap::Butt);
                context.set_line_width(width + 2.0);
                draw()?;
                context.stroke()?;

                context.set_line_cap(cairo::LineCap::Square);
                context.set_operator(cairo::Operator::Clear);
                context.set_line_width(width + 0.8);
                draw()?;
                context.stroke()?;

                context.pop_group_to_source()?;
                context.paint()?;

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

        let is_in_route = if zoom <= 12 {
            row.get_bool("is_in_route")?
        } else {
            false
        };

        let trail_visibility = 0.666f64.powf(row.get_i32("trail_visibility")? as f64);

        match (zoom, class, typ) {
            (14.., _, "pier") => {
                apply_highway_defaults(2.0);
                context.set_source_color(colors::PIER);
                draw()?;
            }
            (12.., "railway", "rail") if ["main", ""].contains(&service) => {
                draw_rail(colors::RAIL, 1.5, 5.0, 9.5, 1.0)?;
            }
            (13.., "railway", _)
                if ["light_rail", "tram"].contains(&typ)
                    || typ == "rail" && service != "main" && !service.is_empty() =>
            {
                draw_rail(colors::TRAM, 1.0, 4.5, 9.5, 1.0)?;
            }
            (
                13..,
                "railway",
                "miniature" | "monorail" | "funicular" | "narrow_gauge" | "subway",
            ) => {
                draw_rail(colors::TRAM, 1.0, 4.5, 7.5, 1.0)?;
            }
            (14.., "railway", "construction") => {
                draw_rail(colors::RAILWAY_DISUSED, 1.0, 4.5, 7.5, 1.0)?;

                apply_highway_defaults(1.5 + 1.0 / 3.0);
                context.set_source_color(colors::CONSTRUCTION_ROAD_1);
                context.set_dash(&[5.0, 5.0], 0.0);
                draw()?;

                context.set_source_color(colors::CONSTRUCTION_ROAD_2);
                context.set_dash(&[5.0, 5.0], 5.0);
                draw()?;
            }
            (14.., "railway", "disused" | "preserved") => {
                draw_rail(colors::RAILWAY_DISUSED, 1.0, 4.5, 7.5, 1.0)?;
            }
            (8..=11, "railway", "rail") if ["main", ""].contains(&service) => {
                let koef = 0.8 * 1.15f64.powf((zoom - 8) as f64);

                draw_rail(
                    colors::RAIL,
                    koef,
                    10.0 / 3.0 * koef,
                    9.5 / 1.5 * koef,
                    0.5 * koef,
                )?;
            }
            (8..=11, "highway", "motorway" | "trunk" | "motorway_link" | "trunk_link") => {
                apply_highway_defaults(0.8 * highway_width_coef());
                draw()?;
            }
            (8..=11, "highway", "primary" | "primary_link") => {
                apply_highway_defaults(0.7 * highway_width_coef());
                draw()?;
            }
            (8..=11, "highway", "secondary" | "secondary_link") => {
                apply_highway_defaults(0.6 * highway_width_coef());
                draw()?;
            }
            (8..=11, "highway", "tertiary" | "tertiary_link") => {
                apply_highway_defaults(0.5 * highway_width_coef());
                draw()?;
            }
            (12.., "highway", "motorway" | "trunk") => {
                apply_highway_defaults(2.5);
                context.set_source_color(colors::SUPERROAD);
                draw()?;

                draw_bridges_tunnels(2.5 + 1.0)?;
            }
            (12.., "highway", "motorway_link" | "trunk_link") => {
                apply_highway_defaults(1.5 + 2.0 / 3.0);
                context.set_source_color(colors::SUPERROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 2.0 / 3.0 + 1.0)?;
            }
            (12.., "highway", "primary") => {
                apply_highway_defaults(1.5 + 2.0 / 3.0);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 2.0 / 3.0 + 1.0)?;
            }
            (12.., "highway", "primary_link" | "secondary") => {
                apply_highway_defaults(1.5 + 1.0 / 3.0);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 1.0 / 3.0 + 1.0)?;
            }
            (12.., "highway", "construction") => {
                apply_highway_defaults(1.5 + 1.0 / 3.0);
                context.set_source_color(colors::CONSTRUCTION_ROAD_1);
                context.set_dash(&[5.0, 5.0], 0.0);
                draw()?;

                context.set_source_color(colors::CONSTRUCTION_ROAD_2);
                context.set_dash(&[5.0, 5.0], 5.0);
                draw()?;
            }
            (12.., "highway", "secondary_link" | "tertiary" | "tertiary_link") => {
                apply_highway_defaults(1.5);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.5 + 1.0 / 3.0 + 1.0)?;
            }
            (12..=13, "highway", "living_street" | "residential" | "unclassified" | "road") => {
                apply_highway_defaults(1.0);
                draw()?;

                draw_bridges_tunnels(1.0 + 1.0)?;
            }
            (14.., "highway", "living_street" | "residential" | "unclassified" | "road") => {
                apply_highway_defaults(1.0);
                context.set_source_color(colors::ROAD);
                draw()?;

                draw_bridges_tunnels(1.0 + 1.0)?;
            }
            (14.., "attraction", "water_slide") => {
                apply_highway_defaults(1.5);
                context.set_source_color(colors::WATER_SLIDE);
                draw()?;

                draw_bridges_tunnels(1.5 + 1.0)?;
            }
            (14.., "highway", "service") if service == "parking_aisle" => {
                apply_highway_defaults(1.0);
                draw()?;

                draw_bridges_tunnels(1.0 + 1.0)?;
            }
            (14.., _, "raceway") | (14.., "leisure", "track") => {
                apply_highway_defaults(1.2);
                context.set_dash(&[9.5, 1.5], 0.0);
                draw()?;

                draw_bridges_tunnels(1.2 + 1.0)?;
            }
            (14.., "highway", "piste") => {
                apply_highway_defaults(1.2);
                context.set_source_color(colors::PISTE);
                draw()?;

                draw_bridges_tunnels(1.2 + 1.0)?;
            }
            (14.., "highway", "footway" | "pedestrian")
            | (14.., "highway" | "railway" | "public_transport", "platform") => {
                apply_highway_defaults(1.0);
                context.set_dash(&[4.0, 2.0], 0.0);
                draw()?;

                draw_bridges_tunnels(1.0 + 1.0)?;
            }
            (14.., "highway", "steps") => {
                apply_highway_defaults(2.5);
                context.set_dash(&[1.0, 2.0], 2.0);
                draw()?;
            }
            (12.., "highway", _)
                if typ == "service" && service != "parking_aisle"
                    || ["escape", "corridor", "bus_guideway"].contains(&typ) =>
            {
                let width = ke() * 1.2;

                apply_highway_defaults(width);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            (12.., "highway", "path")
                if bicycle == "designated"
                    && foot == "designated"
                    && (zoom > 12 || is_in_route) =>
            {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[4.0, 2.0], 0.0);
                context.set_source_color_a(colors::CYCLEWAY, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            (12.., "highway", _)
                if (typ == "cycleway"
                    || typ == "path" && bicycle == "designated" && foot != "designated")
                    && (zoom > 12 || is_in_route) =>
            {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[6.0, 3.0], 0.0);
                context.set_source_color_a(colors::CYCLEWAY, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            (12.., "highway", "path")
                if (bicycle != "designated" || foot == "designated")
                    && (zoom > 12 || is_in_route) =>
            {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[3.0, 3.0], 0.0);
                context.set_source_color_a(colors::TRACK, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            (12.., "highway", "bridleway") if zoom > 12 || is_in_route => {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[6.0, 3.0], 0.0);
                context.set_source_color_a(colors::BRIDLEWAY, trail_visibility);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            (12.., "highway", "via_ferrata") if zoom > 12 || is_in_route => {
                let width = ke();

                apply_highway_defaults(width);
                context.set_dash(&[4.0, 4.0], 0.0);
                draw()?;

                draw_bridges_tunnels(width + 1.0)?;
            }
            (12.., "highway", "track") if (zoom > 12 || is_in_route || tracktype == "grade1") => {
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

            _ => (),
        };

        let oneway = row.get_i16("oneway")?;

        if zoom >= 14 && oneway != 0 {
            path_line_string(context, geom);

            let path = context.copy_path()?;

            context.new_path();

            draw_markers_on_path(&path, 50.0, 100.0, &|x, y, angle| -> cairo::Result<()> {
                context.save()?;
                context.translate(x, y);
                context.rotate(angle + if oneway < 0 { 180.0 } else { 0.0 });
                context.set_source_surface(arrow, -rect.width() / 2.0, -rect.height() / 2.0)?;
                context.paint()?;
                context.restore()?;

                Ok(())
            })?;
        }
    }

    context.restore()?;

    Ok(())
}
