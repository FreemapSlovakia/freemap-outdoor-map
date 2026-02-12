use std::{collections::HashMap, sync::LazyLock};

use super::landcover_z_order::build_landcover_z_order_case;
use crate::render::{
    colors::{self, Color, ContextExt, *},
    ctx::Ctx,
    draw::path_geom::{path_geometry, path_line_string_with_offset, walk_geometry_line_strings},
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::SvgRepo,
    xyz::to_absolute_pixel_coords,
};
use cairo::{Extend, Matrix, SurfacePattern};
use postgres::Client;

pub enum Paint {
    Fill(Color),
    Pattern(&'static str),
    Stroke(f64, Color),
}

#[rustfmt::skip]
pub(crate) const PAINT_DEFS: &[(&[&str], &[Paint])] = &[
    (&["allotments"], &[Paint::Fill(ALLOTMENTS)]),
    (&["cemetery", "grave_yard"], &[Paint::Fill(GRASSY), Paint::Stroke(2.0, BLACK), Paint::Pattern("grave")]),
    (&["clearcut"], &[Paint::Pattern("clearcut2")]),
    (&["bare_rock"], &[Paint::Pattern("bare_rock")]),
    (&["beach"], &[Paint::Fill(BEACH), Paint::Pattern("sand")]),
    (&["brownfield"], &[Paint::Fill(BROWNFIELD)]),
    (&["bog"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("bog")]),
    (&["mangrove"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("mangrove")]),
    (&["college", "school", "university"], &[Paint::Fill(COLLEGE)]),
    (&["commercial", "retail"], &[Paint::Fill(COMMERCIAL)]),
    (&["dam", "weir"], &[Paint::Fill(DAM)]),
    (&["farmland"], &[Paint::Fill(FARMLAND)]),
    (&["farmyard"], &[Paint::Fill(FARMYARD), Paint::Stroke(2.0, BLACK)]),
    (&["fell", "grass", "grassland"], &[Paint::Fill(GRASSY)]),
    (&["marsh", "wet_meadow", "fen"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("marsh")]),
    (&["footway", "garages", "pedestrian", "railway"], &[Paint::Fill(NONE)]),
    (&["forest", "wood"], &[Paint::Fill(FOREST)]),
    (&["garden", "park"], &[Paint::Fill(ORCHARD), Paint::Stroke(2.0, BLACK)]),
    (&["heath"], &[Paint::Fill(HEATH)]),
    (&["hospital"], &[Paint::Fill(HOSPITAL)]),
    (&["industrial", "wastewater_plant"], &[Paint::Fill(INDUSTRIAL)]),
    (&["landfill"], &[Paint::Fill(LANDFILL)]),
    (&["residential"], &[Paint::Fill(RESIDENTIAL)]),
    (&["meadow", "village_green"], &[Paint::Fill(GRASSY)]),
    (&["orchard"], &[Paint::Fill(ORCHARD), Paint::Pattern("orchard")]),
    (&["dog_park"], &[Paint::Fill(GRASSY), Paint::Pattern("dog_park"), Paint::Stroke(2.0, BLACK)]),
    (&["parking"], &[Paint::Fill(PARKING), Paint::Stroke(2.0, PARKING_STROKE)]),
    (&["pitch", "playground", "golf_course", "track"], &[Paint::Fill(PITCH), Paint::Stroke(2.0, PITCH_STROKE)]),
    (&["plant_nursery"], &[Paint::Fill(SCRUB), Paint::Pattern("plant_nursery")]),
    (&["quarry"], &[Paint::Fill(QUARRY), Paint::Pattern("quarry")]),
    (&["glacier"], &[Paint::Fill(GLACIER), Paint::Pattern("glacier")]),
    (&["reedbed"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("reedbed")]),
    (&["recreation_ground"], &[Paint::Fill(RECREATION_GROUND)]),
    (&["silo"], &[Paint::Fill(SILO), Paint::Stroke(2.0, SILO_STROKE)]),
    (&["scree"], &[Paint::Fill(SCREE), Paint::Pattern("scree")]),
    (&["scrub"], &[Paint::Fill(SCRUB), Paint::Pattern("scrub")]),
    (&["swamp"], &[Paint::Fill(GRASSY), Paint::Pattern("wetland"), Paint::Pattern("swamp")]),
    (&["vineyard"], &[Paint::Fill(ORCHARD), Paint::Pattern("grapes")]),
    (&["wetland"], &[Paint::Pattern("wetland")]),
    (&["winter_sports"], &[]), // NOTE handled separately
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

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("landcover::render");

    let context = ctx.context;

    let min = ctx.bbox.min();

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("landcovers", || {
        let a = "'pitch', 'playground', 'golf_course', 'track'";

        let excl_types = match zoom {
            ..12 => &format!("type NOT IN ({a}) AND"),
            12..13 => {
                &format!("type NOT IN ({a}, 'parking', 'bunker_silo', 'storage_tank', 'silo') AND")
            }
            _ => "",
        };

        let table_suffix = match zoom {
            ..=9 => "_gen0",
            10..=11 => "_gen1",
            12.. => "",
        };

        let z_order_case = build_landcover_z_order_case("type");

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
                osm_id,
                {z_order_case} AS z_order
            FROM
                osm_landcovers{table_suffix}
            WHERE
                {excl_types}
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                z_order DESC NULLS LAST,
                osm_id
        ");

        client.query(query, &ctx.bbox_query_params(Some(4.0)).as_params())
    })?;

    context.save()?;

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        let typ = row.get_string("type")?;

        if let Some(paints) = PAINTS.get(typ) {
            context.push_group();

            for paint in paints.iter() {
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
                        path_geometry(context, &geom);

                        context.set_line_cap(cairo::LineCap::Square);
                        context.set_operator(cairo::Operator::Atop);
                        context.stroke()?;
                    }
                }
            }

            context.pop_group_to_source()?;

            context.paint()?;
        }

        if typ == "winter_sports" && zoom >= 11 {
            let wb = 0.5f64.mul_add(zoom as f64 - 10.0, 2.0);

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

            context.pop_group_to_source().expect("group in source");
            context.paint_with_alpha(0.66)?;
        }
    }

    context.restore()?;

    Ok(())
}
