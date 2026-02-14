use crate::render::{
    collision::Collision,
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        text::{TextOptions, draw_text},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use pangocairo::pango::Weight;
use postgres::Client;

pub fn render(
    ctx: &Ctx,
    client: &mut Client,
    collision: &mut Option<&mut Collision>,
) -> LayerRenderResult {
    let _span = tracy_client::span!("place_names::render");

    let zoom = ctx.zoom;

    let scale = 2.5 * 1.2f64.powf(zoom as f64);

    let rows = ctx.legend_features("place_names", || {
        let by_zoom = match zoom {
            8 => "type = 'city'",
            9..=10 => "(type = 'city' OR type = 'town')",
            11 => "(type = 'city' OR type = 'town' OR type = 'village')",
            12.. => "type <> 'locality'",
            _ => return Ok(Vec::new()),
        };

        #[cfg_attr(any(), rustfmt::skip)]
        let sql = format!("
            SELECT
                name,
                type,
                geometry
            FROM
                osm_places
            WHERE
                {by_zoom} AND
                 name <> '' AND
                 geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            ORDER BY
                z_order DESC,
                population DESC,
                osm_id
        ");

        client.query(&sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
    })?;

    for row in rows {
        let (size, uppercase, halo_width) = match (zoom, row.get_string("type")?) {
            (6.., "city") => (1.2, true, 2.0),
            (9.., "town") => (0.8, true, 2.0),
            (11.., "village") => (0.55, true, 1.5),
            (12.., "hamlet" | "allotments" | "suburb") => (0.50, false, 1.5),
            (14.., "isolated_dwelling" | "quarter") => (0.45, false, 1.5),
            (15.., "neighbourhood") => (0.40, false, 1.5),
            (16.., "farm" | "borough" | "square") => (0.35, false, 1.5),
            _ => continue,
        };

        draw_text(
            ctx.context,
            collision.as_deref_mut(),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            row.get_string("name")?,
            &TextOptions {
                flo: FontAndLayoutOptions {
                    size: size * scale,
                    uppercase,
                    narrow: true,
                    weight: Weight::Bold,
                    letter_spacing: 1.0,
                    ..FontAndLayoutOptions::default()
                },
                halo_width,
                halo_opacity: 0.9,
                alpha: if zoom <= 14 { 1.0 } else { 0.5 },
                ..TextOptions::default()
            },
        )?;
    }

    Ok(())
}
