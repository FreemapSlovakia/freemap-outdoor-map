use crate::render::{
    Feature,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            type,
            geometry
        FROM
            osm_buildings
        WHERE
            geometry && ST_MakeEnvelope($1, $2, $3, $4, 3857)
    ";

    client
        .query(sql, &ctx.bbox_query_params(None).as_params())
        .await
}

pub fn render(ctx: &Ctx, context: &Context, rows: Vec<Feature>) -> LayerRenderResult {
    let _span = tracy_client::span!("buildings::render");

    context.save()?;

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        path_geometry(context, &geom);

        let typ = row.get_string("type")?;

        if typ.starts_with("disused:") || typ == "disused" {
            draw_dashed(context, Some(0.66))?;
        } else if typ.starts_with("abandoned:") || typ == "abandoned" {
            draw_dashed(context, Some(0.33))?;
        } else if typ.starts_with("ruins:") || typ == "ruins" {
            draw_dashed(context, None)?;
        } else {
            context.set_source_color(colors::BUILDING);
            context.fill()?;
        }
    }

    context.restore()?;

    Ok(())
}

/// A translucent fill and dashed outline, kept inside the outline by a clip. A
/// `DestIn` group would do the same, but the PDF backend rasterizes the whole
/// page for it.
fn draw_dashed(context: &Context, fill_alpha: Option<f64>) -> Result<(), cairo::Error> {
    context.save()?;
    context.clip_preserve();

    if let Some(alpha) = fill_alpha {
        context.set_source_color_a(colors::BUILDING, alpha);
        context.fill_preserve()?;
    }

    context.set_dash(&[3.0, 3.0], 0.0);
    context.set_line_width(2.0);
    context.set_source_color(colors::BUILDING);
    context.stroke()?;

    context.restore()
}
