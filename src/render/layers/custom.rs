use crate::render::{
    colors::parse_hex_rgb,
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        path_geom::{path_geometry, path_polygons, walk_geometry_points},
        text::{TextOptions, draw_text},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
};
use cairo::{LineCap, LineJoin};
use geo::{Geometry, InteriorPoint, Transform, Translate};
use geojson::Feature;
use proj::Proj;
use serde_json::Value;

pub fn render(ctx: &Ctx, features: &Vec<Feature>) -> LayerRenderResult {
    let context = ctx.context;

    // TODO lazy
    let proj =
        Proj::new_known_crs("EPSG:4326", "EPSG:3857", None).expect("projection 4326 -> 3857");

    context.save()?;

    context.set_line_join(LineJoin::Round);
    context.set_line_cap(LineCap::Round);

    for feature in features {
        let mut geom: Geometry = Geometry::try_from(feature.clone())?;

        geom.transform(&proj).unwrap();

        let geom = geom.project_to_tile(&ctx.tile_projector);

        let mut width = 1f64;
        let mut r = 1f64;
        let mut g = 0f64;
        let mut b = 1f64;

        let mut name: Option<String> = None;

        if let Some(ref properties) = feature.properties {
            if let Some(Value::String(color)) = properties.get("color")
                && let Some((cr, cg, cb)) = parse_hex_rgb(color)
            {
                r = cr;
                g = cg;
                b = cb;
            }

            if let Some(Value::String(n)) = properties.get("name")
                && !n.is_empty()
            {
                name.replace(n.clone());
            }

            if let Some(Value::Number(a)) = properties.get("width")
                && let Some(b) = a.as_f64()
            {
                width = b;
            }
        }

        path_geometry(ctx.context, &geom);

        context.set_line_width(width);

        context.set_source_rgb(r, g, b);

        context.stroke()?;

        path_polygons(ctx.context, &geom);

        context.set_source_rgba(r, g, b, 0.25);

        context.fill()?;

        context.set_source_rgb(r, g, b);

        walk_geometry_points(&geom, &mut |point| -> cairo::Result<()> {
            let x = point.x();
            let y = point.y();
            let r = 10f64;
            let h = r * 2.2;
            let dy = r * r / h;
            let tx_sq = r * r - dy * dy;
            let tx = tx_sq.max(0.0).sqrt();

            context.new_sub_path();
            context.move_to(x, y);
            context.line_to(x - tx, y + (dy - h));
            context.arc(x, y - h, r, dy.atan2(-tx), dy.atan2(tx));
            context.line_to(x, y);
            context.close_path();

            context.fill()?;

            Ok(())
        })?;

        if let Some(name) = name {
            let point = if let Geometry::Point(point) = geom {
                point.translate(0.0, -44.0)
            } else if let Some(point) = geom.interior_point() {
                point
            } else {
                continue;
            };

            let _ = draw_text(
                context,
                None,
                &point,
                &name,
                &TextOptions {
                    flo: FontAndLayoutOptions {
                        size: 15.0,
                        ..Default::default()
                    },
                    halo_width: 2.0,
                    ..Default::default()
                },
            );
        }
    }

    context.restore()?;

    Ok(())
}
