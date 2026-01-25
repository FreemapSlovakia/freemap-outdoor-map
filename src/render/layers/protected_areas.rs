use crate::render::{
    SvgRepo,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        hatch::hatch_geometry,
        line_pattern::draw_line_pattern,
        path_geom::{path_geometry, path_line_string_with_offset, walk_geometry_line_strings},
    },
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("protected_areas::render");

    let zoom = ctx.zoom;

    let sql = &format!(
        "SELECT type, protect_class, geometry FROM osm_protected_areas WHERE geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) {}",
        if zoom < 12 {
            " AND NOT (type = 'nature_reserve' OR type = 'protected_area' AND protect_class <> '2')"
        } else {
            ""
        }
    );

    let rows = &client.query(sql, &ctx.bbox_query_params(Some(10.0)).as_params())?;

    let geometries: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            geometry_geometry(row)
                .map(|geom| (geom.project_to_tile(&ctx.tile_projector), geom, row))
        })
        .collect();

    let context = ctx.context;

    // hatching
    if zoom <= 11 {
        context.save()?;

        for (projected, unprojected, row) in &geometries {
            let typ: &str = row.get("type");
            let protect_class: &str = row.get("protect_class");

            if typ == "national_park" || typ == "protected_area" && protect_class == "2" {
                context.push_group();

                path_geometry(context, projected);

                context.clip();

                context.set_source_color_a(colors::PROTECTED, if zoom < 11 { 0.5 } else { 0.4 });
                context.set_dash(&[], 0.0);
                context.set_line_width(0.7);

                hatch_geometry(ctx, unprojected, 3.0, -45.0)?;

                context.stroke()?;

                context.pop_group_to_source()?;
                context.paint()?;
            }
        }

        context.restore()?;
    }

    // NOTE we do ST_Intersection to prevent memory error for very long borders on bigger zooms

    let sql = &format!(
        "SELECT
            type, protect_class, ST_Intersection(geometry, ST_Expand(ST_MakeEnvelope($6, $7, $8, $9, 3857), 50000)) AS geometry
        FROM osm_protected_areas
        WHERE geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) {}",
        if zoom < 12 {
            " AND NOT (type = 'nature_reserve' OR type = 'protected_area' AND protect_class <> '2')"
        } else {
            ""
        }
    );

    let snap = (26f64 - ctx.zoom as f64).exp2();

    let rows = &client.query(
        sql,
        &ctx.bbox_query_params(Some(10.0))
            .push(((ctx.bbox.min().x - snap) / snap).floor() * snap)
            .push(((ctx.bbox.min().y - snap) / snap).floor() * snap)
            .push(((ctx.bbox.max().x + snap) / snap).ceil() * snap)
            .push(((ctx.bbox.max().y + snap) / snap).ceil() * snap)
            .as_params(),
    )?;

    let geometries: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            geometry_geometry(row)
                .map(|geom| (geom.project_to_tile(&ctx.tile_projector), geom, row))
        })
        .collect();

    // border
    for (projected, _, row) in &geometries {
        let typ: &str = row.get("type");
        let protect_class: &str = row.get("protect_class");

        if typ == "nature_reserve" || typ == "protected_area" && protect_class != "2" {
            let sample = svg_repo.get("protected_area")?;

            walk_geometry_line_strings(projected, &mut |line_string| {
                draw_line_pattern(ctx, line_string, 0.8, sample)
            })?;
        }
    }

    context.push_group();

    for (projected, _, row) in &geometries {
        let typ: &str = row.get("type");
        let protect_class: &str = row.get("protect_class");

        if typ == "national_park" || typ == "protected_area" && protect_class == "2" {
            let wb = if zoom > 10 {
                0.5f64.mul_add(zoom as f64 - 10.0, 2.0)
            } else {
                2.0
            };

            context.set_source_color(colors::PROTECTED);
            context.set_dash(&[], 0.0);
            context.set_line_width(wb * 0.75);
            context.set_line_join(cairo::LineJoin::Round);
            path_geometry(context, projected);
            context.stroke()?;

            context.set_line_width(wb);
            context.set_source_color_a(colors::PROTECTED, 0.5);
            walk_geometry_line_strings(projected, &mut |iter| {
                path_line_string_with_offset(context, iter, wb * 0.75);

                cairo::Result::Ok(())
            })?;
            context.stroke()?;
        }
    }

    context.pop_group_to_source()?;

    context.paint_with_alpha(0.66)?;

    Ok(())
}
