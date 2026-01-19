use crate::render::{
    SvgRepo,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{line_pattern::draw_line_pattern, path_geom::path_line_string},
    layer_render_error::LayerRenderResult,
    layers::{hillshading, hillshading_datasets::HillshadingDatasets},
    projectable::{TileProjectable, geometry_line_string},
};
use postgres::Client;

pub fn render(
    ctx: &Ctx,
    client: &mut Client,
    svg_repo: &mut SvgRepo,
    hillshading_datasets: &mut Option<HillshadingDatasets>,
    shading: bool,
    hillshade_scale: f64,
) -> LayerRenderResult {
    let _span = tracy_client::span!("feature_lines_maskable::render");

    let sql = "
        SELECT geometry, type
        FROM osm_feature_lines
        WHERE
            type NOT IN ('cutline', 'valley', 'ridge') AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)";

    let rows = client.query(sql, &ctx.bbox_query_params(Some(8.0)).as_params())?;

    if rows.is_empty() {
        return Ok(());
    }

    let mut mask_surfaces = Vec::new();

    if shading && let Some(hillshading_datasets) = hillshading_datasets {
        for cc in [
            "pl", "sk", "cz", "at", /*"ch", "it" (CH, IT are not so detailed) */
        ] {
            let mask_surface = hillshading::load_surface(
                ctx,
                cc,
                hillshading_datasets,
                hillshade_scale,
                hillshading::Mode::Mask,
            )?;

            if let Some(mask_surface) = mask_surface {
                mask_surfaces.push(mask_surface);
            }
        }
    }

    if hillshading::mask_covers_tile(&mut mask_surfaces)? {
        return Ok(());
    }

    let context = ctx.context;

    let mut draw = || -> LayerRenderResult {
        for row in &rows {
            let geom = geometry_line_string(row).project_to_tile(&ctx.tile_projector);

            match row.get("type") {
                "earth_bank" => {
                    draw_line_pattern(ctx, &geom, 0.8, svg_repo.get("earth_bank")?)?;
                }
                "dyke" => {
                    draw_line_pattern(ctx, &geom, 0.8, svg_repo.get("dyke")?)?;
                }
                "embankment" => {
                    draw_line_pattern(ctx, &geom, 0.8, svg_repo.get("embankment-half")?)?;
                }
                "gully" => {
                    draw_line_pattern(ctx, &geom, 0.8, svg_repo.get("gully")?)?;
                }
                "cliff" => {
                    draw_line_pattern(ctx, &geom, 0.8, svg_repo.get("cliff")?)?;

                    context.set_source_color(colors::AREA_LABEL);
                    context.set_line_width(1.0);
                    path_line_string(context, &geom);
                    context.stroke()?;
                }
                _ => {
                    //
                }
            }
        }

        Ok(())
    };

    if mask_surfaces.is_empty() {
        draw()?;
    } else {
        context.push_group();

        draw()?;

        context.push_group();

        for mask_surface in &mask_surfaces {
            hillshading::paint_surface(ctx, mask_surface, hillshade_scale, 1.0)?;
        }

        context.pop_group_to_source()?;

        context.set_operator(cairo::Operator::DestOut);
        context.paint()?;

        context.pop_group_to_source()?;
        context.paint()?;
    }

    Ok(())
}
