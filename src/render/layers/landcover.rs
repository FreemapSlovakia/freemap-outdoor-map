use crate::render::{
    Feature,
    colors::{
        self, ALLOTMENTS, BEACH, BLACK, BROWNFIELD, COLLEGE, COMMERCIAL, Color, ContextExt, DAM,
        FARMLAND, FARMYARD, FOREST, GLACIER, GRASSY, HEATH, HOSPITAL, INDUSTRIAL, LANDFILL, NONE,
        ORCHARD, PARKING, PARKING_STROKE, PITCH, PITCH_STROKE, QUARRY, RECREATION_GROUND,
        RESIDENTIAL, SCREE, SCRUB, SILO, SILO_STROKE, TREE,
    },
    ctx::Ctx,
    draw::path_geom::{path_geometry, path_line_string_with_offset, walk_geometry_line_strings},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::SvgRepo,
    xyz::to_absolute_pixel_coords,
};
use cairo::{Context, Extend, Matrix, SurfacePattern};
use std::{collections::HashMap, sync::LazyLock};

pub enum Paint {
    Fill(Color),
    Pattern(&'static str),
    Stroke(f64, Color),
}

#[rustfmt::skip]
pub const PAINT_DEFS: &[(&[&str], &[Paint])] = &[
    (&["forest", "wood", "tree_group"], &[Paint::Fill(FOREST)]),
    (&["meadow", "village_green", "fell", "grass", "grassland"], &[Paint::Fill(GRASSY)]),
    (&["flowerbed"], &[Paint::Fill(GRASSY), Paint::Pattern("flowerbed")]),
    (&["scrub"], &[Paint::Fill(SCRUB), Paint::Pattern("scrub")]),
    (&["heath"], &[Paint::Fill(HEATH)]),
    (&["bare_rock"], &[Paint::Pattern("bare_rock")]),
    (&["glacier"], &[Paint::Fill(GLACIER), Paint::Pattern("glacier")]),
    (&["wetland"], &[Paint::Pattern("wetland")]),
    (&["mangrove"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("mangrove")]),
    (&["bog"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("bog")]),
    (&["swamp"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("swamp")]),
    (&["marsh", "wet_meadow", "fen"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("marsh")]),
    (&["reedbed"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("reedbed")]),
    (&["scree"], &[Paint::Fill(SCREE), Paint::Pattern("scree")]),
    (&["farmland"], &[Paint::Fill(FARMLAND)]),
    (&["farmyard"], &[Paint::Fill(FARMYARD), Paint::Stroke(2.0, BLACK)]),
    (&["beach"], &[Paint::Fill(BEACH), Paint::Pattern("sand")]),

    (&["vineyard"], &[Paint::Fill(ORCHARD), Paint::Pattern("grapes")]),
    (&["orchard"], &[Paint::Fill(ORCHARD), Paint::Pattern("orchard")]),
    (&["plant_nursery"], &[Paint::Fill(SCRUB), Paint::Pattern("plant_nursery")]),
    (&["clearcut"], &[Paint::Pattern("clearcut2")]),
    (&["quarry"], &[Paint::Fill(QUARRY), Paint::Pattern("quarry")]),

    (&["residential"], &[Paint::Fill(RESIDENTIAL)]),
    (&["commercial", "retail"], &[Paint::Fill(COMMERCIAL)]),
    (&["industrial", "wastewater_plant"], &[Paint::Fill(INDUSTRIAL)]),
    (&["brownfield"], &[Paint::Fill(BROWNFIELD)]),
    (&["landfill"], &[Paint::Fill(LANDFILL)]),
    (&["dam", "weir"], &[Paint::Fill(DAM)]),

    (&["hospital"], &[Paint::Fill(HOSPITAL)]),
    (&["allotments"], &[Paint::Fill(ALLOTMENTS)]),
    (&["garden", "park"], &[Paint::Fill(ORCHARD), Paint::Stroke(2.0, BLACK)]),
    (&["pitch", "playground", "golf_course", "track"], &[Paint::Fill(PITCH), Paint::Stroke(2.0, PITCH_STROKE)]),
    (&["dog_park"], &[Paint::Fill(GRASSY), Paint::Pattern("dog_park"), Paint::Stroke(2.0, BLACK)]),
    (&["cemetery", "grave_yard"], &[Paint::Fill(GRASSY), Paint::Stroke(2.0, BLACK), Paint::Pattern("grave")]),
    (&["college", "school", "university"], &[Paint::Fill(COLLEGE)]),
    (&["footway", "garages", "pedestrian", "railway"], &[Paint::Fill(NONE)]),

    (&["parking"], &[Paint::Fill(PARKING), Paint::Stroke(2.0, PARKING_STROKE)]),
    (&["recreation_ground"], &[Paint::Fill(RECREATION_GROUND)]),
    (&["winter_sports"], &[]), // NOTE handled separately
    (&["bunker_silo"], &[Paint::Fill(SILO), Paint::Stroke(2.0, SILO_STROKE)]),
];

pub static PAINTS: LazyLock<HashMap<&'static str, &'static [Paint]>> = LazyLock::new(|| {
    let mut paint_map = HashMap::new();

    for (types, paints) in PAINT_DEFS {
        for &typ in *types {
            paint_map.insert(typ, *paints);
        }
    }

    paint_map
});

/// Drawn above all other landcovers: they are mapped on top of the ground they
/// lie in, and the pattern-only ones would vanish under a smaller fill.
/// Everything else nests by area: smaller on top, as OSM Carto does.
const OVERLAY_TYPES: &str = "'wetland', 'bare_rock', 'clearcut'";

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let a = "'pitch', 'playground', 'golf_course', 'track'";

    let excl_types = match ctx.zoom {
        ..12 => &format!("type NOT IN ({a}, 'parking', 'bunker_silo') AND"),
        12..13 => &format!("type NOT IN ({a}) AND"),
        _ => "",
    };

    let table_suffix = match ctx.zoom {
        ..=9 => "_gen0",
        10..=11 => "_gen1",
        12.. => "",
    };

    let query = &format!("
        SELECT
            CASE
                WHEN
                    type = 'wetland' AND
                    tags->'wetland' IN ('bog', 'reedbed', 'marsh', 'swamp', 'wet_meadow', 'mangrove', 'fen')
                THEN tags->'wetland'
                ELSE type
            END AS type,
            geometry,
            osm_id
        FROM
            osm_landcovers{table_suffix}
        WHERE
            {excl_types}
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            type IN ({OVERLAY_TYPES}),
            area DESC,
            osm_id,
            -- a way tagged with several landcovers comes once per type, with equal area
            type
    ");

    client
        .query(query, &ctx.bbox_query_params(Some(4.0)).as_params())
        .await
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: &[Feature],
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("landcover::render");

    let min = ctx.bbox.min();

    let zoom = ctx.zoom;

    context.save()?;

    for row in rows {
        let typ = row.get_string("type")?;

        if matches!(typ, "forest_boundary" | "forest_compartment") {
            continue;
        }

        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        if let Some(paints) = PAINTS.get(typ) {
            if paints.len() > 1 {
                context.push_group();
            }

            for paint in *paints {
                match paint {
                    Paint::Fill(color) => {
                        context.set_source_color(*color);
                        path_geometry(context, &geom);
                        context.fill()?;
                    }
                    Paint::Pattern(pattern) => {
                        let tile = svg_repo.get(pattern)?;

                        let pattern = SurfacePattern::create(tile);

                        let (x, y) = to_absolute_pixel_coords(min.x, min.y, zoom);

                        let rect = tile.extents().expect("tile extents");

                        let mut matrix = Matrix::identity();
                        matrix.translate((x % rect.width()).round(), (y % rect.height()).round());
                        pattern.set_matrix(matrix);

                        pattern.set_extend(Extend::Repeat);

                        context.set_source(&pattern)?;

                        path_geometry(context, &geom);

                        context.fill()?;
                    }
                    Paint::Stroke(width, color) => {
                        if matches!(
                            typ,
                            "garden" | "park" | "cemetery" | "dog_park" | "farmyard"
                        ) {
                            context.set_source_color_a(*color, 0.2);
                        } else {
                            context.set_source_color(*color);
                        }

                        context.set_line_width(*width);
                        context.set_line_cap(cairo::LineCap::Square);

                        // Clip rather than `Atop`, which Cairo's PDF backend rasterizes;
                        // the same result, as every fill under a stroke is opaque.
                        context.save()?;
                        path_geometry(context, &geom);
                        context.clip_preserve();
                        context.stroke()?;
                        context.restore()?;
                    }
                }
            }

            if paints.len() > 1 {
                context.pop_group_to_source()?;
                context.paint()?;
            }
        }
    }

    context.restore()?;

    if ctx.zoom >= 14 {
        context.push_group();

        context.set_source_color(TREE);
        context.set_line_width(if ctx.zoom == 14 { 4.0 } else { 6.0 });
        context.set_line_cap(cairo::LineCap::Square);

        for row in rows {
            let typ = row.get_string("type")?;

            if !matches!(typ, "forest_boundary" | "forest_compartment") {
                continue;
            }

            let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

            path_geometry(context, &geom);

            context.stroke()?;
        }

        context.pop_group_to_source()?;

        context.paint_with_alpha(0.2)?;
    }

    Ok(())
}

/// Ski resort (`landuse=winter_sports`) boundaries.
///
/// Drawn in a separate, much later stage than the landcover fills so that lines painted
/// on top of the landcovers - most notably cutlines, which often run along the very
/// same ways - don't cover the boundary.
pub fn render_winter_sports_boundaries(
    ctx: &Ctx,
    context: &Context,
    rows: &[Feature],
) -> LayerRenderResult {
    let _span = tracy_client::span!("landcover::render_winter_sports_boundaries");

    let wb = 0.5f64.mul_add(ctx.zoom as f64 - 10.0, 2.0);

    context.save()?;

    for row in rows {
        if row.get_string("type")? != "winter_sports" {
            continue;
        }

        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        context.push_group();

        context.set_source_color(colors::WATER);
        context.set_dash(&[], 0.0);
        context.set_line_width(wb * 0.75);
        context.set_line_cap(cairo::LineCap::Square);

        path_geometry(context, &geom);
        context.stroke()?;

        context.set_line_width(wb);
        context.set_source_color_a(colors::WATER, 0.5);
        walk_geometry_line_strings(&geom, &mut |iter| {
            path_line_string_with_offset(context, iter, wb * 0.75);

            cairo::Result::Ok(())
        })?;
        context.stroke()?;

        context.pop_group_to_source()?;
        context.paint_with_alpha(0.66)?;
    }

    context.restore()?;

    Ok(())
}
