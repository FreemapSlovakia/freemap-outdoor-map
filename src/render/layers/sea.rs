use crate::render::{
    Feature, GeomError,
    colors::{self, ContextExt},
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    projectable::TileProjectable,
};
use cairo::Context;
use geo::Geometry;

pub async fn query(ctx: &Ctx, client: &tokio_postgres::Client, margin_px: f64) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let table = match ctx.zoom {
        ..=7 => "land_z5_7",
        8..=10 => "land_z8_10",
        11..=13 => "land_z11_13",
        14.. => "land_z14_plus",
    };

    #[cfg_attr(any(), rustfmt::skip)]
        let sql = format!("
            SELECT
                ST_Intersection(
                    ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5),
                    ST_Buffer(geometry, $6)
                ) AS geometry
            FROM
                {table}
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ");

    client.query(
        &sql,
        &ctx.bbox_query_params(Some(margin_px))
            .push((20.0 - ctx.zoom as f64).exp2() / 25.0)
            .as_params(),
    ).await
}

pub fn project(ctx: &Ctx, rows: &[Feature]) -> Result<Vec<Geometry>, LayerRenderError> {
    let mut land = Vec::with_capacity(rows.len());

    for row in rows {
        let geom = match row.get_geometry() {
            Ok(geom) => geom.project_to_tile(&ctx.tile_projector),
            Err(err) => match err {
                crate::render::FeatureError::GeomError(GeomError::GeomIsEmpty) => continue, // NOTE sea is often empty
                _ => Err(err)?,
            },
        };

        land.push(geom);
    }

    Ok(land)
}

pub fn render(context: &Context, land: &[Geometry]) -> LayerRenderResult {
    let _span = tracy_client::span!("sea::render");

    context.save()?;

    context.set_source_color(colors::WATER);
    context.paint()?;

    context.set_source_color(colors::WHITE);

    for geom in land {
        path_geometry(context, geom);

        context.fill()?;
    }

    context.restore()?;

    Ok(())
}
