use crate::render::{
    Feature,
    categories::Category,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{
        line_pattern::{draw_line_pattern, draw_line_pattern_scaled},
        markers_on_path::draw_markers_on_path,
        path_geom::path_line_string,
    },
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::{HillshadingDatasets, hillshading},
    projectable::TileProjectable,
    svg_repo::{Options, SvgRepo},
};
use cairo::Context;

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
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

    // Obstacles are not listed in `types`: their concrete values are open-ended
    // (mapping.yaml maps unknown `obstacle=*` to `obstacle_yes`), so they are matched
    // by prefix. Only stage 5 draws them, hence its zoom gate here.
    let obstacles = if ctx.zoom >= 14 {
        " OR type LIKE 'obstacle_%'"
    } else {
        ""
    };

    let sql = &format!("
        SELECT
            geometry,
            CASE
                WHEN type IN ('obstacle_tree', 'obstacle_vegetation')
                THEN type
                WHEN type LIKE 'obstacle_%'
                THEN 'obstacle'
                ELSE type
            END AS type,
            under = '1' AS under
        FROM
            osm_feature_lines
        WHERE
            (type = ANY($6){obstacles})
            AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
    ");

    client
        .query(
            sql,
            &ctx.bbox_query_params(Some(8.0)).push(types).as_params(),
        )
        .await
}

/// Hillshading inputs used to hide feature lines that the shading already conveys.
pub struct HillshadingMask<'a> {
    pub datasets: &'a mut HillshadingDatasets,
    /// Countries whose hillshading masks cut the lines out.
    pub countries: &'a [String],
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    stage: u8,
    rows: &[Feature],
    svg_repo: &mut SvgRepo,
    mask: Option<HillshadingMask<'_>>,
) -> LayerRenderResult {
    let _span = tracy_client::span!("feature_lines::render");

    let mut draw = |maskable: bool| -> Result<bool, LayerRenderError> {
        let mut touched = false;

        for row in rows {
            let geom = row.get_line_string()?.project_to_tile(&ctx.tile_projector);

            context.save()?;

            let zoom = ctx.zoom;

            let mut untouched = false;

            let typ = row.get_string("type")?;

            match (stage, zoom, typ, maskable) {
                (1, 13.., "cutline", false) => {
                    path_line_string(context, &geom);

                    context.set_source_color(colors::SCRUB);
                    context.set_dash(&[], 0.0);
                    context.set_line_width(0.33f64.mul_add(((zoom - 12) as f64).exp2(), 2.0));
                    context.stroke()?;
                }
                (2, 12.., "pipeline", false) => {
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

                    context.paint_with_alpha(if row.get_bool("under")? { 0.33 } else { 1.0 })?;
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
                        context,
                        ctx.size,
                        &geom,
                        0.8,
                        (2.0 + (zoom as f64 - 15.0).exp2()) / 4.5,
                        svg_repo.get("tree2")?,
                    )?;
                }
                (2, 15.., "earth_bank", true) => {
                    draw_line_pattern(context, ctx.size, &geom, 0.8, svg_repo.get("earth_bank")?)?;
                }
                (2, 15.., "dyke", true) => {
                    draw_line_pattern(context, ctx.size, &geom, 0.8, svg_repo.get("dyke")?)?;
                }
                (2, 15.., "embankment", true) => {
                    draw_line_pattern(
                        context,
                        ctx.size,
                        &geom,
                        0.8,
                        svg_repo.get("embankment-half")?,
                    )?;
                }
                (2, 15.., "gully", true) => {
                    draw_line_pattern(context, ctx.size, &geom, 0.8, svg_repo.get("gully")?)?;
                }
                (2, 15.., "cliff", true) => {
                    draw_line_pattern(context, ctx.size, &geom, 0.8, svg_repo.get("cliff")?)?;

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
                (5, 14.., "obstacle" | "obstacle_tree" | "obstacle_vegetation", false) => {
                    path_line_string(context, &geom);

                    let path = context.copy_path_flat()?;

                    context.new_path();

                    // The POI layer draws these same icons, and the files carry no
                    // colour of their own - so the tint has to be asked for here too, or
                    // a line obstacle draws black beside a red node obstacle on one tile.
                    // Taken from the category both layers share, so they cannot drift.
                    let surface = svg_repo.get_with(Options {
                        names: vec![typ.to_string()],
                        stylesheet: Some(format!(
                            "path {{ fill: {} }}",
                            colors::rgb_hex(Category::Terrain.icon_color())
                        )),
                        halo: true,
                        use_extents: false,
                        ..Default::default()
                    })?;

                    // NOTE: use ink_extents() rather than extents(): the surface is an
                    // *unbounded* recording surface, for which extents() returns None.
                    let (ox, oy, w, h) = surface.ink_extents();

                    let hw = ox + w / 2.0;

                    let hh = oy + h / 2.0;

                    draw_markers_on_path(&path, 50.0, 100.0, &|x, y, _angle| {
                        context.save()?;
                        context.translate((x - hw).round(), (y - hh).round());
                        context.set_source_surface(surface, 0.0, 0.0)?;
                        context.paint()?;
                        context.restore()?;
                        Ok(())
                    })?;
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

    if let Some(HillshadingMask {
        datasets: hillshading_datasets,
        countries,
    }) = mask
    {
        let mut mask_surfaces = Vec::new();

        for cc in countries {
            let mask_surface =
                hillshading::load_surface(ctx, cc, hillshading_datasets, hillshading::Mode::Mask)?;

            if let Some(mask_surface) = mask_surface {
                mask_surfaces.push(mask_surface);
            }
        }

        if mask_surfaces.is_empty() {
            draw(true)?;

            return Ok(());
        } else if hillshading::mask_covers_tile(&mut mask_surfaces.iter_mut().collect::<Vec<_>>())?
        {
            return Ok(());
        }

        context.push_group();

        if !draw(true)? {
            context.pop_group()?;

            return Ok(());
        }

        context.push_group();

        for mask_surface in &mask_surfaces {
            hillshading::paint_surface(ctx, context, mask_surface, 1.0)?;
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
#[cfg(test)]
mod tests {
    use crate::render::{categories::Category, colors, layers::POIS};

    /// The obstacle icons are drawn by this layer for lines and by the POI layer for
    /// nodes, and carry no colour of their own - so both callers must ask for the same
    /// tint. They drew black here and red there once the colour moved out of the SVGs.
    #[test]
    fn line_obstacles_are_tinted_like_the_poi_ones() {
        for typ in ["obstacle", "obstacle_tree", "obstacle_vegetation"] {
            let def = POIS
                .get(typ)
                .and_then(|defs| defs.first())
                .unwrap_or_else(|| panic!("{typ} has no definition"));

            assert_eq!(
                colors::rgb_hex(def.color()),
                colors::rgb_hex(Category::Terrain.icon_color()),
                "{typ}: this layer tints from Category::Terrain, so the POI layer's \
                 colour for it must come from there too - an `Extra::color` override \
                 here would make one layer draw it in a different colour"
            );
        }
    }
}
