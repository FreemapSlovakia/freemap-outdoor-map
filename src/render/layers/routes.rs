use crate::render::{
    collision::Collision,
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        line_pattern::draw_line_pattern_scaled,
        offset_line::offset_line_string,
        path_geom::{path_line_string_with_offset, walk_geometry_line_strings},
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::{LayerRenderError, LayerRenderResult},
    projectable::TileProjectable,
    svg_repo::{Options, SvgRepo},
};
use bitflags::bitflags;
use colorsys::{Rgb, RgbRatio};
use postgres::Client;

const COLOR_SQL: &str = r#"
  CASE
    WHEN "osmc:symbol" LIKE 'red:%' THEN 0
    WHEN "osmc:symbol" LIKE 'blue:%' THEN 1
    WHEN "osmc:symbol" LIKE 'green:%' THEN 2
    WHEN "osmc:symbol" LIKE 'yellow:%' THEN 3
    WHEN "osmc:symbol" LIKE 'black:%' THEN 4
    WHEN "osmc:symbol" LIKE 'white:%' THEN 5
    WHEN "osmc:symbol" LIKE 'orange:%' THEN 6
    WHEN "osmc:symbol" LIKE 'violet:%' THEN 7
    WHEN "osmc:symbol" LIKE 'purple:%' THEN 7
    WHEN colour = 'red' THEN 0
    WHEN colour = 'blue' THEN 1
    WHEN colour = 'green' THEN 2
    WHEN colour = 'yellow' THEN 3
    WHEN colour = 'black' THEN 4
    WHEN colour = 'white' THEN 5
    WHEN colour = 'orange' THEN 6
    WHEN colour = 'violet' THEN 7
    WHEN colour = 'purple' THEN 7
    ELSE 8
  END
"#;

const COLORS: [(&str, &str); 9] = [
    ("none", "a0a0a0"),
    ("purple", "c000c0"),
    ("orange", "ff8000"),
    ("white", "ffffff"),
    ("black", "000000"),
    ("yellow", "f0f000"),
    ("green", "00a000"),
    ("blue", "5050ff"),
    ("red", "ff3030"),
];

bitflags! {
  #[derive(Debug, Clone, Copy)]
  pub struct RouteTypes: u32 {
      const HIKING = 0b0000_0001;
      const HORSE = 0b0000_0010;
      const BICYCLE = 0b0000_0100;
      const SKI = 0b0000_1000;
  }
}

fn format_vec(vec: &[&str]) -> String {
    if vec.is_empty() {
        "'_x_'".to_string()
    } else {
        vec.iter()
            .map(|&item| format!("'{}'", item))
            .collect::<Vec<String>>()
            .join(",")
    }
}

fn get_routes_query(
    route_types: &RouteTypes,
    include_networks: Option<Vec<&str>>,
    gen_suffix: &str,
) -> String {
    let mut lefts = Vec::<&str>::new();

    let mut rights = Vec::<&str>::new();

    if route_types.contains(RouteTypes::HIKING) {
        lefts.extend_from_slice(&["hiking", "foot", "running"]);
    }

    if route_types.contains(RouteTypes::HORSE) {
        lefts.push("horse");
    }

    if route_types.contains(RouteTypes::BICYCLE) {
        rights.extend_from_slice(&["bicycle", "mtb"]);
    }

    if route_types.contains(RouteTypes::SKI) {
        rights.extend_from_slice(&["ski", "piste"]);
    }

    let lefts_in = format_vec(&lefts);

    let rights_in = format_vec(&rights);

    let cond = match include_networks {
        None => String::from(""),
        Some(networks) => {
            let mut result = String::from("network IN (");

            for (i, &network) in networks.iter().enumerate() {
                if i > 0 {
                    result.push(',');
                }
                result.push('\'');
                result.push_str(network);
                result.push('\'');
            }

            result.push_str(") AND ");

            result
        }
    };

    let bool_horse = route_types.contains(RouteTypes::HORSE);
    let bool_hiking = route_types.contains(RouteTypes::HIKING);
    let bool_bicycle = route_types.contains(RouteTypes::BICYCLE);
    let bool_ski = route_types.contains(RouteTypes::SKI);

    format!("
      SELECT
        ST_Multi(ST_LineMerge(ST_Collect(geometry))) AS geometry,
        idx(arr1, 0) AS h_red,
        idx(arr1, 1) AS h_blue,
        idx(arr1, 2) AS h_green,
        idx(arr1, 3) AS h_yellow,
        idx(arr1, 4) AS h_black,
        idx(arr1, 5) AS h_white,
        idx(arr1, 6) AS h_orange,
        idx(arr1, 7) AS h_purple,
        idx(arr1, 8) AS h_none,
        idx(arr1, 10) AS h_red_loc,
        idx(arr1, 11) AS h_blue_loc,
        idx(arr1, 12) AS h_green_loc,
        idx(arr1, 13) AS h_yellow_loc,
        idx(arr1, 14) AS h_black_loc,
        idx(arr1, 15) AS h_white_loc,
        idx(arr1, 16) AS h_orange_loc,
        idx(arr1, 17) AS h_purple_loc,
        idx(arr1, 18) AS h_none_loc,
        idx(arr2, 20) AS b_red,
        idx(arr2, 21) AS b_blue,
        idx(arr2, 22) AS b_green,
        idx(arr2, 23) AS b_yellow,
        idx(arr2, 24) AS b_black,
        idx(arr2, 25) AS b_white,
        idx(arr2, 26) AS b_orange,
        idx(arr2, 27) AS b_purple,
        idx(arr2, 28) AS b_none,
        idx(arr2, 30) AS s_red,
        idx(arr2, 31) AS s_blue,
        idx(arr2, 32) AS s_green,
        idx(arr2, 33) AS s_yellow,
        idx(arr2, 34) AS s_black,
        idx(arr2, 35) AS s_white,
        idx(arr2, 36) AS s_orange,
        idx(arr2, 37) AS s_purple,
        idx(arr2, 38) AS s_none,
        idx(arr1, 40) AS r_red,
        idx(arr1, 41) AS r_blue,
        idx(arr1, 42) AS r_green,
        idx(arr1, 43) AS r_yellow,
        idx(arr1, 44) AS r_black,
        idx(arr1, 45) AS r_white,
        idx(arr1, 46) AS r_orange,
        idx(arr1, 47) AS r_purple,
        idx(arr1, 48) AS r_none,
        refs1,
        refs2,
        icount(arr1 - array[1000, 1010, 1020, 1030, 1040]) AS off1,
        icount(arr2 - array[1000, 1010, 1020, 1030, 1040]) AS off2
      FROM (
        SELECT
          array_to_string(
            array(
              SELECT distinct itm FROM unnest(
                array_agg(
                  CASE
                    WHEN
                      osm_routes.type IN ({lefts_in})
                    THEN
                      CASE
                        WHEN name <> '' AND ref <> ''
                        THEN name || ' (' || ref || ')'
                        ELSE COALESCE(NULLIF(name, ''), NULLIF(ref, '')) END
                    ELSE
                      null
                    END
                )
              ) AS itm ORDER BY itm
            ),
            ', '
          ) AS refs1,
          array_to_string(
            array(
              SELECT distinct itm FROM unnest(
                array_agg(
                  CASE
                    WHEN
                      osm_routes.type IN ({rights_in})
                    THEN
                      CASE
                        WHEN name <> '' AND ref <> ''
                        THEN name || ' (' || ref || ')'
                        ELSE COALESCE(NULLIF(name, ''), NULLIF(ref, '')) END
                    ELSE
                      null
                    END
                )
              ) AS itm ORDER BY itm
            ),
            ', '
          ) AS refs2,
          first(geometry) AS geometry,
          uniq(sort(array_agg(
            CASE
              WHEN osm_routes.type IN ({lefts_in}) THEN
                CASE
                  WHEN {bool_horse} AND osm_routes.type = 'horse' THEN 40
                  WHEN {bool_hiking} AND osm_routes.type IN ('hiking', 'foot', 'running') THEN (CASE WHEN network IN ('iwn', 'nwn', 'rwn') THEN 0 ELSE 10 END)
                  ELSE 1000
                END +
                {COLOR_SQL}
              ELSE 1000
            END
          ))) AS arr1,
          uniq(sort(array_agg(
            CASE
              WHEN osm_routes.type IN ({rights_in}) THEN
                CASE
                  WHEN {bool_bicycle} AND osm_routes.type IN ('bicycle', 'mtb') THEN 20
                  WHEN {bool_ski} AND osm_routes.type IN ('ski', 'piste') THEN 30
                  ELSE 1000
                END +
                {COLOR_SQL}
              ELSE
                1000
              END
          ))) AS arr2
        FROM osm_route_members{gen_suffix} JOIN osm_routes ON (osm_route_members{gen_suffix}.osm_id = osm_routes.osm_id AND state <> 'proposed')
        WHERE {cond}geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        GROUP BY member
      ) AS aaa
      GROUP BY
        h_red, h_blue, h_green, h_yellow, h_black, h_white, h_orange, h_purple, h_none,
        h_red_loc, h_blue_loc, h_green_loc, h_yellow_loc, h_black_loc, h_white_loc, h_orange_loc, h_purple_loc, h_none_loc,
        b_red, b_blue, b_green, b_yellow, b_black, b_white, b_orange, b_purple, b_none,
        s_red, s_blue, s_green, s_yellow, s_black, s_white, s_orange, s_purple, s_none,
        r_red, r_blue, r_green, r_yellow, r_black, r_white, r_orange, r_purple, r_none,
        off1, off2, refs1, refs2")
}

pub fn render_marking(
    ctx: &Ctx,
    client: &mut Client,
    route_types: &RouteTypes,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("routes::render_marking");

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("routes", || {
        let sql = match zoom {
            9 => get_routes_query(route_types, Some(vec!["iwn", "icn"]), "_gen0"),
            10 => get_routes_query(route_types, Some(vec!["iwn", "nwn", "icn", "ncn"]), "_gen1"),
            11 => get_routes_query(
                route_types,
                Some(vec!["iwn", "nwn", "rwn", "icn", "ncn", "rcn"]),
                "_gen1",
            ),
            12..=13 => get_routes_query(route_types, None, ""),
            14.. => get_routes_query(route_types, None, ""),
            _ => return Ok(Vec::new()),
        };

        client.query(&sql, &ctx.bbox_query_params(Some(512.0)).as_params())
    })?;

    for row in rows {
        let geom = row.geometry()?.project_to_tile(&ctx.tile_projector);

        let (zo, wf) = match zoom {
            ..=11 => (1.0, 1.5),
            12 => (2.0, 1.5),
            13.. => (3.0, 2.0),
        }; // offset from highway

        let df = 1.25;

        for color in COLORS.iter() {
            if route_types.contains(RouteTypes::HORSE) {
                let off = row.get_i32(&format!("r_{}", color.0))?;

                if off > 0 {
                    let offset = ((off as f64 - 1.0) * wf).mul_add(df, zo) + 0.5;

                    let sample = svg_repo.get_extra(
                        &format!("horse-{}", color.1),
                        Some(|| Options {
                            names: vec!["horse".into()],
                            stylesheet: Some(format!("path {{ fill: #{} }}", color.1)),
                            ..Default::default()
                        }),
                    )?;

                    walk_geometry_line_strings(&geom, &mut |part| {
                        draw_line_pattern_scaled(
                            ctx.context,
                            ctx.size,
                            &offset_line_string(part, offset),
                            0.5,
                            wf / 2.0,
                            sample,
                        )
                    })?;
                }
            }

            if route_types.contains(RouteTypes::SKI) {
                let off = row.get_i32(&format!("s_{}", color.0))?;

                if off > 0 {
                    let offset = -((off as f64 - 1.0) * wf).mul_add(2.0, zo) - 1.0;

                    let pattern = svg_repo.get_extra(
                        &format!("ski-{}", color.1),
                        Some(|| Options {
                            names: vec!["ski".into()],
                            stylesheet: Some(format!("path {{ fill: #{} }}", color.1)),
                            ..Default::default()
                        }),
                    )?;

                    walk_geometry_line_strings::<_, LayerRenderError>(&geom, &mut |part| {
                        draw_line_pattern_scaled(
                            ctx.context,
                            ctx.size,
                            &offset_line_string(part, offset),
                            0.5,
                            wf / 2.0,
                            pattern,
                        )?;

                        Ok(())
                    })?;
                }
            }

            let context = ctx.context;

            if route_types.contains(RouteTypes::BICYCLE) {
                let off = row.get_i32(&format!("b_{}", color.0))?;

                if off > 0 {
                    let offset = -((off as f64 - 1.0) * wf).mul_add(2.0, zo) - 1.0;

                    context.save()?;

                    walk_geometry_line_strings(&geom, &mut |part| {
                        path_line_string_with_offset(context, part, offset);

                        cairo::Result::Ok(())
                    })?;

                    context.set_line_width(wf * 2.0);
                    context.set_line_join(cairo::LineJoin::Round);
                    context.set_line_cap(cairo::LineCap::Round);

                    let rgb: RgbRatio = Rgb::from_hex_str(color.1).expect("color").as_ratio();
                    context.set_source_rgb(rgb.r(), rgb.g(), rgb.b());
                    context.set_dash(&[0.001, wf * 3.0], 0.0);

                    context.stroke()?;

                    context.restore()?;
                }
            }

            if route_types.contains(RouteTypes::HIKING) {
                {
                    let off = row.get_i32(&format!("h_{}", color.0))?;

                    if off > 0 {
                        let offset = ((off as f64 - 1.0) * wf).mul_add(df, zo) + 0.5;

                        context.save()?;

                        walk_geometry_line_strings(&geom, &mut |part| {
                            path_line_string_with_offset(context, part, offset);

                            cairo::Result::Ok(())
                        })?;

                        context.set_line_width(wf);
                        context.set_line_join(cairo::LineJoin::Round);
                        context.set_line_cap(cairo::LineCap::Butt);
                        let rgb: RgbRatio = Rgb::from_hex_str(color.1).expect("color").as_ratio();
                        context.set_source_rgb(rgb.r(), rgb.g(), rgb.b());
                        context.set_dash(&[], 0.0);

                        context.stroke()?;

                        context.restore()?;
                    }
                }

                {
                    let off = row.get_i32(&format!("h_{}_loc", color.0))?;

                    if off > 0 {
                        let offset = ((off as f64 - 1.0) * wf).mul_add(df, zo) + 0.5;

                        context.save()?;

                        walk_geometry_line_strings(&geom, &mut |part| {
                            path_line_string_with_offset(context, part, offset);

                            cairo::Result::Ok(())
                        })?;

                        context.set_line_width(wf);
                        context.set_line_join(cairo::LineJoin::Round);
                        context.set_line_cap(cairo::LineCap::Butt);
                        let rgb: RgbRatio = Rgb::from_hex_str(color.1).expect("color").as_ratio();
                        context.set_source_rgb(rgb.r(), rgb.g(), rgb.b());
                        context.set_dash(&[wf * 3.0, wf], 0.0);

                        context.stroke()?;

                        context.restore()?;
                    }
                }
            }
        }
    }

    Ok(())
}

pub fn render_labels(
    ctx: &Ctx,
    client: &mut Client,
    route_types: &RouteTypes,
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("routes::render_labels");

    let rows = ctx.legend_features("routes", || {
        let query = get_routes_query(route_types, None, "");

        client.query(&query, &ctx.bbox_query_params(Some(2048.0)).as_params())
    })?;

    for row in rows {
        let geom = row.geometry()?.project_to_tile(&ctx.tile_projector);

        let refs1 = row.get_string("refs1")?;
        let off1 = row.get_i32("off1")?;

        let refs2 = row.get_string("refs2")?;
        let off2 = row.get_i32("off2")?;

        walk_geometry_line_strings(&geom, &mut |geom| {
            let mut options = TextOnLineOptions {
                flo: FontAndLayoutOptions {
                    size: 11.0,
                    ..Default::default()
                },
                halo_opacity: 0.2,
                distribution: Distribution::Align {
                    align: Align::Center,
                    repeat: Repeat::Spaced(500.0),
                },
                keep_offset_side: true,
                ..Default::default()
            };

            for (refs, offset) in [
                (refs1, -f64::from(off1).mul_add(2.5, 9.0)),
                (refs2, f64::from(off2).mul_add(2.5, 10.0)),
            ] {
                options.offset = offset;

                let _drawn = draw_text_on_line(ctx.context, geom, refs, Some(collision), &options)?;
            }

            cairo::Result::Ok(())
        })?;
    }

    Ok(())
}
