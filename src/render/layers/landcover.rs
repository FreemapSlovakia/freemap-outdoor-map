use super::landcover_z_order::build_landcover_z_order_case;
use crate::render::{
    colors::{self, Color, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::{TileProjectable, geometry_geometry},
    svg_repo::SvgRepo,
    xyz::to_absolute_pixel_coords,
};
use cairo::{Extend, Matrix, SurfacePattern};
use postgres::Client;

pub fn render(ctx: &Ctx, client: &mut Client, svg_repo: &mut SvgRepo) -> LayerRenderResult {
    let _span = tracy_client::span!("landcover::render");

    let context = ctx.context;
    let min = ctx.bbox.min();

    let rows = {
        let a = "'pitch', 'playground', 'golf_course', 'track'";

        let excl_types = match ctx.zoom {
            ..12 => &format!("type NOT IN ({a}) AND"),
            12..13 => {
                &format!("type NOT IN ({a}, 'parking', 'bunker_silo', 'storage_tank', 'silo') AND")
            }
            _ => "",
        };

        let table_suffix = match ctx.zoom {
            ..=9 => "_gen0",
            10..=11 => "_gen1",
            12.. => "",
        };

        let z_order_case = build_landcover_z_order_case("type");

        let query = &format!("
            SELECT
                CASE
                    WHEN type = 'wetland' AND tags->'wetland' IN ('bog', 'reedbed', 'marsh', 'swamp', 'wet_meadow', 'mangrove', 'fen')
                    THEN tags->'wetland'
                    ELSE type
                END AS type,
                ST_Intersection(ST_MakeValid(geometry), ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), 100)) AS geometry,
                osm_id,
                {z_order_case} AS z_order
            FROM
                osm_landcovers{table_suffix}
            WHERE
                {excl_types}
                geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)
            ORDER BY
                z_order DESC NULLS LAST,
                osm_id
        ");

        client.query(query, &ctx.bbox_query_params(None).as_params())?
    };

    context.save()?;

    for row in rows {
        let Some(geom) =
            geometry_geometry(&row).map(|geom| geom.project_to_tile(&ctx.tile_projector))
        else {
            continue;
        };

        let colour_area = |color: Color| -> cairo::Result<()> {
            context.set_source_color(color);
            path_geometry(context, &geom);
            context.fill()?;

            Ok(())
        };

        let mut pattern_area = |path: &str| -> LayerRenderResult {
            let tile = svg_repo.get(path)?;

            let pattern = SurfacePattern::create(tile);

            let (x, y) = to_absolute_pixel_coords(min.x, min.y, ctx.zoom);

            let rect = tile.extents().expect("tile extents");

            let mut matrix = Matrix::identity();
            matrix.translate((x % rect.width()).round(), (y % rect.height()).round());
            pattern.set_matrix(matrix);

            pattern.set_extend(Extend::Repeat);

            context.set_source(&pattern)?;

            path_geometry(context, &geom);

            context.fill()?;

            Ok(())
        };

        let typ: &str = row.get("type");

        match typ {
            "allotments" => {
                colour_area(colors::ALLOTMENTS)?;
            }
            "cemetery" | "grave_yard" => {
                colour_area(colors::GRASSY)?;
                pattern_area("grave")?;
            }
            "clearcut" => {
                pattern_area("clearcut2")?;
            }
            "bare_rock" => {
                pattern_area("bare_rock")?;
            }
            "beach" => {
                colour_area(colors::BEACH)?;
                pattern_area("sand")?;
            }
            "brownfield" => {
                colour_area(colors::BROWNFIELD)?;
            }
            "bog" => {
                colour_area(colors::GRASSY)?;
                pattern_area("wetland")?;
                pattern_area("bog")?;
            }
            "college" => {
                colour_area(colors::COLLEGE)?;
            }
            "commercial" => {
                colour_area(colors::COMMERCIAL)?;
            }
            "dam" => {
                colour_area(colors::DAM)?;
            }
            "farmland" => {
                colour_area(colors::FARMLAND)?;
            }
            "farmyard" => {
                colour_area(colors::FARMYARD)?;
            }
            "fell" => {
                colour_area(colors::GRASSY)?;
            }
            "marsh" | "wet_meadow" | "fen" => {
                colour_area(colors::GRASSY)?;
                pattern_area("wetland")?;
                pattern_area("marsh")?;
            }
            "footway" => {
                colour_area(colors::NONE)?;
            }
            "forest" => {
                colour_area(colors::FOREST)?;

                context.set_source_rgb(0.0, 0.0, 0.0);
                context.set_line_width(1.0);
            }
            "garages" => {
                colour_area(colors::NONE)?;
            }
            "grass" => {
                colour_area(colors::GRASSY)?;
            }
            "garden" => {
                colour_area(colors::ORCHARD)?;

                context.set_source_rgba(0.0, 0.0, 0.0, 0.2);
                context.set_line_width(1.0);
                path_geometry(context, &geom);
                context.stroke()?;
            }
            "grassland" => {
                colour_area(colors::GRASSY)?;
            }
            "heath" => {
                colour_area(colors::HEATH)?;
            }
            "hospital" => {
                colour_area(colors::HOSPITAL)?;
            }
            "industrial" => {
                colour_area(colors::INDUSTRIAL)?;
            }
            "landfill" => {
                colour_area(colors::LANDFILL)?;
            }
            "living_street" => {
                colour_area(colors::RESIDENTIAL)?;
            }
            "mangrove" => {
                colour_area(colors::GRASSY)?;
                pattern_area("wetland")?;
                pattern_area("mangrove")?;
            }
            "meadow" => {
                colour_area(colors::GRASSY)?;
            }
            "orchard" => {
                colour_area(colors::ORCHARD)?;
                pattern_area("orchard")?;
            }
            "park" => {
                colour_area(colors::GRASSY)?;
            }
            "dog_park" => {
                colour_area(colors::GRASSY)?;
                pattern_area("dog_park")?;
            }
            "parking" => {
                colour_area(colors::PARKING)?;

                context.set_source_color(colors::PARKING_STROKE);
                context.set_line_width(1.0);
                path_geometry(context, &geom);
                context.stroke()?;
            }
            "pedestrian" => {
                colour_area(colors::NONE)?;
            }
            "pitch" | "playground" | "golf_course" | "track" => {
                colour_area(colors::PITCH)?;

                context.set_source_color(colors::PITCH_STROKE);
                context.set_line_width(1.0);
                path_geometry(context, &geom);
                context.stroke()?;
            }
            "plant_nursery" => {
                colour_area(colors::SCRUB)?;
                pattern_area("plant_nursery")?;
            }
            "quarry" => {
                colour_area(colors::QUARRY)?;
                pattern_area("quarry")?;
            }
            "glacier" => {
                colour_area(colors::GLACIER)?;
                pattern_area("glacier")?;
            }
            "railway" => {
                colour_area(colors::NONE)?;
            }
            "reedbed" => {
                colour_area(colors::GRASSY)?;
                pattern_area("wetland")?;
                pattern_area("reedbed")?;
            }
            "recreation_ground" => {
                colour_area(colors::RECREATION_GROUND)?;
            }
            "residential" => {
                colour_area(colors::RESIDENTIAL)?;
            }
            "retail" => {
                colour_area(colors::COMMERCIAL)?;
            }
            "silo" => {
                colour_area(colors::SILO)?;

                context.set_source_color(colors::SILO_STROKE);
                context.set_line_width(1.0);
                path_geometry(context, &geom);
                context.stroke()?;
            }
            "school" => {
                colour_area(colors::COLLEGE)?;
            }
            "scree" | "blockfield" => {
                colour_area(colors::SCREE)?;
                pattern_area("scree")?;
            }
            "scrub" => {
                colour_area(colors::SCRUB)?;
                pattern_area("scrub")?;
            }
            "swamp" => {
                colour_area(colors::GRASSY)?;
                pattern_area("wetland")?;
                pattern_area("swamp")?;
            }
            "university" => {
                colour_area(colors::COLLEGE)?;
            }
            "village_green" => {
                colour_area(colors::GRASSY)?;
            }
            "vineyard" => {
                colour_area(colors::ORCHARD)?;
                pattern_area("grapes")?;
            }
            "wastewater_plant" => {
                colour_area(colors::INDUSTRIAL)?;
            }
            "weir" => {
                colour_area(colors::DAM)?;
            }
            "wetland" => {
                pattern_area("wetland")?;
            }
            "wood" => {
                colour_area(colors::FOREST)?;
            }
            _ => (),
        }
    }

    context.restore()?;

    Ok(())
}
