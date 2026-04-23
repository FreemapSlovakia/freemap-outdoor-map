use crate::render::{
    Feature,
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        path_geom::walk_geometry_line_strings,
        text::{TextOptions, draw_text},
        text_on_line::{Align, Distribution, Repeat, TextOnLineOptions, draw_text_on_line},
    },
    layer_render_error::LayerRenderResult,
    layers::national_park_names::REPLACEMENTS,
    projectable::TileProjectable,
    regex_replacer::replace,
};
use cairo::Context;
use cosmic_text::Style;

pub async fn query_centroids(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            name,
            ST_Centroid(geometry) AS geometry
        FROM
            osm_landcovers
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            (type = 'nature_reserve' OR (type = 'protected_area' AND tags->'protect_class' <> '2'))
        ORDER BY
            area DESC
    ";

    client.query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params()).await
}

pub async fn query_borders(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let sql = "
        SELECT
            type,
            name,
            ST_Boundary(geometry) AS geometry
        FROM
            osm_landcovers
        WHERE
            (type IN ('national_park', 'winter_sports') OR (type = 'protected_area' AND tags->'protect_class' = '2')) AND
            name <> '' AND
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
        ORDER BY
            area DESC
    ";

    client.query(sql, &ctx.bbox_query_params(Some(1024.0)).as_params()).await
}

pub fn render_centroids(
    ctx: &Ctx,
    context: &Context,
    centroids: Vec<Feature>,
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("protected_area_names::render_centroids");

    let text_options = TextOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        halo_opacity: 0.75,
        color: colors::PROTECTED,
        ..TextOptions::default()
    };

    for row in centroids {
        draw_text(
            context,
            Some(collision),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            row.get_string("name")?,
            &text_options,
        )?;
    }

    Ok(())
}

pub fn render_borders(
    ctx: &Ctx,
    context: &Context,
    borders: Vec<Feature>,
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("protected_area_names::render_borders");

    let mut text_options = TextOnLineOptions {
        flo: FontAndLayoutOptions {
            style: Style::Italic,
            ..FontAndLayoutOptions::default()
        },
        alpha: 0.66,
        halo_opacity: 0.75,
        offset: -14.0,
        distribution: Distribution::Align {
            align: Align::Center,
            repeat: Repeat::Spaced(600.0),
        },
        keep_offset_side: true,
        ..TextOnLineOptions::default()
    };

    for row in borders {
        text_options.color = match row.get_string("type")? {
            "national_park" | "protected_area" => colors::PROTECTED,
            "winter_sports" => colors::WATER,
            _ => continue,
        };

        let name = row.get_string("name")?;

        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        walk_geometry_line_strings(&geom, &mut |geom| {
            let _drawn = draw_text_on_line(
                context,
                geom,
                &replace(name, &REPLACEMENTS),
                Some(collision),
                &text_options,
            )?;

            cairo::Result::Ok(())
        })?;
    }

    Ok(())
}
