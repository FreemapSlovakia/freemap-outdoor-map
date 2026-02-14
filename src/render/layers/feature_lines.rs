use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        line_pattern::{draw_line_pattern, draw_line_pattern_scaled},
        path_geom::path_line_string,
    },
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::{HillshadingDatasets, hillshading},
    projectable::TileProjectable,
    svg_repo::SvgRepo,
};
use postgres::Client;

pub fn query(ctx: &Ctx, client: &mut Client) -> Result<Vec<Feature>, postgres::Error> {
    ctx.legend_features("feature_lines", || {
        let mut types = vec![];

        if ctx.zoom >= 11 {
            types.extend(["runway", "taxiway", "parking_position", "taxilane"]);
        }

        if ctx.zoom >= 12 {
            types.extend([
                "cable_car",
                "chair_lift",
                "drag_lift",
                "gondola",
                "goods",
                "j-bar",
                "magic_carpet",
                "mixed_lift",
                "platter",
                "rope_tow",
                "t-bar",
                "zip_line",
                "pipeline",
            ]);
        }

        if ctx.zoom >= 12 {
            types.extend(["cutline", "weir", "dam", "tree_row", "line"]);
        }

        if ctx.zoom >= 14 {
            types.push("minor_line");
        }

        if ctx.zoom >= 15 {
            types.extend(["earth_bank", "dyke", "embankment", "gully", "cliff"]);
        }

        if ctx.zoom >= 16 {
            types.extend([
                "city_wall",
                "hedge",
                "ditch",
                "fence",
                "retaining_wall",
                "wall",
            ]);
        }

        let sql = "
            SELECT
                geometry,
                type,
                tags
            FROM
                osm_feature_lines
            WHERE
                type = ANY($6)
                AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ";

        client.query(
            sql,
            &ctx.bbox_query_params(Some(8.0)).push(types).as_params(),
        )
    })
}

pub fn render(
    ctx: &Ctx,
    stage: u8,
    rows: &[Feature],
    svg_repo: &mut SvgRepo,
    hillshading_datasets: Option<&mut HillshadingDatasets>,
) -> LayerRenderResult {
    let _span = tracy_client::span!("feature_lines::render");

    let context = ctx.context;

    let mut draw = |maskable: bool| -> Result<bool, LayerRenderError> {
        let mut touched = false;

        for row in rows {
            let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

            context.save()?;

            let zoom = ctx.zoom;

            let mut untouched = false;

            match (stage, zoom, row.get_string("type")?, maskable) {
                (1, 13.., "cutline", false) => {
                    for row in rows {
                        let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

                        path_line_string(context, &geom);

                        context.set_source_color(colors::SCRUB);
                        context.set_dash(&[], 0.0);
                        context
                            .set_line_width(0.33f64.mul_add(((ctx.zoom - 12) as f64).exp2(), 2.0));
                        context.stroke_preserve()?;
                        context.stroke()?;
                    }
                }
                (2, 12.., "pipeline", false) => {
                    let tags = row.get_hstore("tags")?;

                    context.push_group();

                    path_line_string(context, &geom);

                    context.set_source_color(colors::PIPELINE);
                    context.set_dash(&[], 0.0);
                    context.set_line_join(cairo::LineJoin::Round);
                    context.set_line_width(2.0);
                    context.stroke_preserve()?;

                    context.set_line_width(4.0);
                    context.set_dash(&[0.0, 15.0, 1.5, 1.5, 1.5, 1.0], 0.0);
                    context.stroke()?;

                    context.pop_group_to_source()?;

                    let location = tags.get("location").unwrap_or(&None).as_deref();

                    let alpha = if matches!(location, Some("underground" | "underwater")) {
                        0.33
                    } else {
                        1.0
                    };

                    context.paint_with_alpha(alpha)?;
                }
                (2, 13.., "weir", false) => {
                    if zoom >= 16 {
                        path_line_string(context, &geom);

                        context.set_dash(&[9.0, 3.0], 0.0);
                        context.set_source_color(colors::DAM_LINE);
                        context.set_line_width(3.0);
                        context.stroke()?;
                    }
                }
                (2, 13.., "dam", false) => {
                    if zoom >= 16 {
                        path_line_string(context, &geom);

                        context.set_source_color(colors::DAM_LINE);
                        context.set_line_width(3.0);
                        context.stroke()?;
                    }
                }
                (2, 13.., "tree_row", false) => {
                    draw_line_pattern_scaled(
                        ctx.context,
                        ctx.size,
                        &geom,
                        0.8,
                        (2.0 + (zoom as f64 - 15.0).exp2()) / 4.5,
                        svg_repo.get("tree2")?,
                    )?;
                }
                (2, 15.., "earth_bank", true) => {
                    draw_line_pattern(
                        ctx.context,
                        ctx.size,
                        &geom,
                        0.8,
                        svg_repo.get("earth_bank")?,
                    )?;
                }
                (2, 15.., "dyke", true) => {
                    draw_line_pattern(ctx.context, ctx.size, &geom, 0.8, svg_repo.get("dyke")?)?;
                }
                (2, 15.., "embankment", true) => {
                    draw_line_pattern(
                        ctx.context,
                        ctx.size,
                        &geom,
                        0.8,
                        svg_repo.get("embankment-half")?,
                    )?;
                }
                (2, 15.., "gully", true) => {
                    draw_line_pattern(ctx.context, ctx.size, &geom, 0.8, svg_repo.get("gully")?)?;
                }
                (2, 15.., "cliff", true) => {
                    draw_line_pattern(ctx.context, ctx.size, &geom, 0.8, svg_repo.get("cliff")?)?;

                    context.set_source_color(colors::AREA_LABEL);
                    context.set_line_width(1.0);
                    path_line_string(context, &geom);
                    context.stroke()?;
                }
                (3, 11.., "runway" | "taxiway" | "parking_position" | "taxilane", false) => {
                    let (way_width, dash_width, dash_array) = match ctx.zoom {
                        11 => (3.0, 0.5, &[3.0, 3.0]),
                        12..=13 => (5.0, 1.0, &[4.0, 4.0]),
                        14.. => (8.0, 1.0, &[6.0, 6.0]),
                        _ => panic!("unsupported zoom"),
                    };

                    path_line_string(context, &geom);

                    context.set_source_color(colors::AEROWAY);
                    context.set_dash(&[], 0.0);
                    context.set_line_width(way_width);
                    context.stroke_preserve()?;

                    context.set_source_rgb(1.0, 1.0, 1.0);
                    context.set_line_width(dash_width);
                    context.set_dash(dash_array, 0.0);
                    context.stroke()?;
                }
                (4, 16.., "city_wall", false) => {
                    path_line_string(context, &geom);

                    context.set_dash(&[], 0.0);
                    context.set_source_color(colors::BUILDING);
                    context.set_line_width(2.0);
                    context.stroke()?;
                }
                (4, 16.., "hedge", false) => {
                    path_line_string(context, &geom);

                    context.set_source_color(colors::PITCH);
                    context.set_line_width(ctx.zoom as f64 - 14.0);
                    context.set_dash(&[0.01, ctx.zoom as f64 - 14.0], 0.0);
                    context.set_line_join(cairo::LineJoin::Round);
                    context.set_line_cap(cairo::LineCap::Round);
                    context.stroke()?;
                }
                (4, 16.., "ditch" | "fence" | "retaining_wall" | "wall", false) => {
                    path_line_string(context, &geom);

                    context.set_dash(&[2.0, 1.0], 0.0);
                    context.set_line_width(1.0);
                    context.set_source_color(colors::BARRIERWAY);
                    context.stroke()?;
                }
                (
                    4,
                    12..,
                    "cable_car" | "chair_lift" | "drag_lift" | "gondola" | "goods" | "j-bar"
                    | "magic_carpet" | "mixed_lift" | "platter" | "rope_tow" | "t-bar" | "zip_line",
                    false,
                ) => {
                    context.push_group();

                    path_line_string(context, &geom);

                    context.set_source_color(colors::BLACK);
                    context.set_line_width(1.0);
                    context.stroke_preserve()?;

                    context.set_dash(&[1.0, 25.0], 0.0);
                    context.set_line_width(5.0);
                    context.stroke()?;

                    context.pop_group_to_source()?;

                    context.paint()?;
                }
                (4, 13.., "line", false) => {
                    path_line_string(context, &geom);

                    context.set_source_color_a(colors::POWER_LINE, 0.5);
                    context.set_line_width(1.0);
                    context.stroke()?;
                }
                (4, 14.., "minor_line", false) => {
                    path_line_string(context, &geom);

                    context.set_source_color_a(colors::POWER_LINE_MINOR, 0.5);
                    context.set_line_width(1.0);
                    context.stroke()?;
                }
                _ => {
                    untouched = true;
                }
            }

            touched = touched || !untouched;

            context.restore()?;
        }

        Ok(touched)
    };

    draw(false)?;

    if let Some(hillshading_datasets) = hillshading_datasets {
        let mut mask_surfaces = Vec::new();

        for cc in [
            "pl", "sk", "cz", "at", /*"ch", "it" (CH, IT are not so detailed) */
        ] {
            let mask_surface =
                hillshading::load_surface(ctx, cc, hillshading_datasets, hillshading::Mode::Mask)?;

            if let Some(mask_surface) = mask_surface {
                mask_surfaces.push(mask_surface);
            }
        }

        if mask_surfaces.is_empty() {
            draw(true)?;

            return Ok(());
        } else if hillshading::mask_covers_tile(&mut mask_surfaces)? {
            return Ok(());
        }

        context.push_group();

        if !draw(true)? {
            context.pop_group()?;

            return Ok(());
        }

        context.push_group();

        for mask_surface in &mask_surfaces {
            hillshading::paint_surface(ctx, mask_surface, 1.0)?;
        }

        context.pop_group_to_source()?;

        context.set_operator(cairo::Operator::DestOut);
        context.paint()?;

        context.pop_group_to_source()?;
        context.paint()?;
    } else {
        draw(true)?;
    }

    Ok(())
}
