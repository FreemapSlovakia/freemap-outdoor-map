use crate::render::{
    Feature,
    collision::Collision,
    colors,
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        text::{TextOptions, draw_text},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::Context;
use pangocairo::pango::Weight;
use postgres::Client;

pub fn query(ctx: &Ctx, client: &mut Client) -> Result<Vec<Feature>, postgres::Error> {
    ctx.legend_features("place_names", || {
        let zoom = ctx.zoom;

        let by_zoom = match zoom {
            8 => "a.type IN('city', 'islet', 'island')",
            9..=10 => "a.type IN('islet', 'island', 'city', 'town')",
            11 => "a.type IN ('islet', 'island', 'city', 'town', 'village')",
            12.. => "a.type <> 'locality'",
            _ => return Ok(Vec::new()),
        };

        #[cfg_attr(any(), rustfmt::skip)]
        let sql = format!("
            SELECT
                a.name,
                a.type,
                COALESCE(a.area, 0) AS area,
                ST_PointOnSurface(a.geometry) AS geometry
            FROM
                osm_places a LEFT JOIN osm_places b ON a.name = b.name AND a.osm_id <> b.osm_id AND ST_Contains(b.geometry, a.geometry)
            WHERE
                 {by_zoom} AND
                 a.name <> '' AND
                 a.geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                 b.osm_id IS NULL
            ORDER BY
                a.z_order DESC,
                a.population DESC,
                a.osm_id
        ");

        client.query(&sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
    })
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    collision: &mut Option<&mut Collision>,
) -> LayerRenderResult {
    let _span = tracy_client::span!("place_names::render");

    let zoom = ctx.zoom;

    let positions = [
        (0.0, -10.0),
        (0.0, 10.0),
        (-30.0, 0.0),
        (30.0, 0.0),
        (-25.0, -8.0),
        (-25.0, 8.0),
        (25.0, -8.0),
        (25.0, 8.0),
    ];

    let scale = 2.5 * 1.2f64.powf(zoom.min(14) as f64);

    for row in rows {
        let mut color = colors::BLACK;
        let mut letter_spacing = 1.0;

        let (size, uppercase, halo_width, italic) = match (zoom, row.get_string("type")?) {
            (8.., "city") => (1.2, true, 2.0, false),
            (9.., "town") => (0.8, true, 2.0, false),
            (11.., "village") => (0.55, true, 1.5, false),
            (12.., "hamlet" | "allotments" | "suburb") => (0.50, false, 1.5, false),
            (14.., "isolated_dwelling" | "quarter") => (0.45, false, 1.5, false),
            (15.., "neighbourhood") => (0.40, false, 1.5, false),
            (16.., "farm" | "borough" | "square") => (0.35, false, 1.5, false),
            (8.., "island" | "islet") => {
                let mut area = row.get_f32("area")?;

                if area == 0.0 {
                    area = 10000.0;
                }

                if area < 4f32.powf(22f32 - zoom as f32) {
                    continue;
                }

                color = colors::LOCALITY_LABEL;
                letter_spacing = 0.0;

                (
                    0.4 * (1.0 + area.sqrt() / 2000.0).min(2.0) as f64,
                    false,
                    1.5,
                    true,
                )
            }
            _ => continue,
        };

        // TODO could be precomputed
        let mut placements = Vec::with_capacity(41);
        placements.push((0.0, 0.0));

        for i in 1..6 {
            for p in positions.iter() {
                placements.push((2.0 * size * p.0 * i as f64, 2.0 * size * p.1 * i as f64));
            }
        }

        draw_text(
            context,
            collision.as_deref_mut(),
            &row.get_point()?.project_to_tile(&ctx.tile_projector),
            row.get_string("name")?,
            &TextOptions {
                flo: FontAndLayoutOptions {
                    size: size * scale,
                    max_width: 8.0 * size * scale,
                    uppercase,
                    narrow: true,
                    weight: Weight::Bold,
                    letter_spacing,
                    style: if italic {
                        pango::Style::Italic
                    } else {
                        pango::Style::Normal
                    },
                    ..FontAndLayoutOptions::default()
                },
                halo_width: halo_width * scale / 30.0,
                halo_opacity: 0.9,
                placements: &placements,
                color,
                ..TextOptions::default()
            },
        )?;
    }

    Ok(())
}
