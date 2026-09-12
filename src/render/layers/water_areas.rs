use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::{hatch::hatch_geometry, path_geom::path_geometry},
    layer_render_error::LayerRenderResult,
    layers::dry_land,
    projectable::TileProjectable,
};
use cairo::Context;
use geo::Geometry;

pub async fn query(ctx: &Ctx, client: &tokio_postgres::Client) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let table_suffix = match ctx.zoom {
        ..=9 => "_gen0",
        10..=11 => "_gen1",
        12.. => "",
    };

    #[cfg_attr(any(), rustfmt::skip)]
        let sql = format!("
            SELECT
                geometry,
                COALESCE(intermittent OR seasonal, false) AS tmp
            FROM
                osm_waterareas{table_suffix}
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ");

    client.query(&sql, &ctx.bbox_query_params(Some(dry_land::KNOWN_MARGIN_PX)).as_params()).await
}

/// Projects the permanent water of `rows` — what [`dry_land`] cuts away — and hands
/// each polygon to `consume`. Intermittent and seasonal beds are left out: they are dry
/// most of the time and carry real terrain.
pub(super) fn for_each_permanent(
    ctx: &Ctx,
    rows: &[Feature],
    mut consume: impl FnMut(&Geometry) -> LayerRenderResult,
) -> LayerRenderResult {
    for row in rows {
        if row.get_bool("tmp")? {
            continue;
        }

        consume(&row.get_geometry()?.project_to_tile(&ctx.tile_projector))?;
    }

    Ok(())
}

pub fn render(ctx: &Ctx, context: &Context, rows: &[Feature]) -> LayerRenderResult {
    let _span = tracy_client::span!("water_areas::render");

    let zoom = ctx.zoom;

    let tile_projector = &ctx.tile_projector;

    context.save()?;

    for row in rows {
        let geom = row.get_geometry()?;

        let projected = geom.project_to_tile(tile_projector);

        let tmp: bool = row.get_bool("tmp")?;

        if tmp {
            context.save()?;

            path_geometry(context, &projected);

            context.clip();

            context.set_source_color(colors::WATER);
            context.paint()?;

            context.set_source_color_a(colors::WHITE, 0.75);
            context.set_dash(&[], 0.0);
            context.set_line_width(2.0);

            hatch_geometry(context, &geom, tile_projector, zoom, 4.0, 0.0)?;

            context.stroke()?;

            context.restore()?;
        } else {
            context.set_source_color(colors::WATER);

            path_geometry(context, &projected);

            context.fill()?;
        }
    }

    context.restore()?;

    Ok(())
}
