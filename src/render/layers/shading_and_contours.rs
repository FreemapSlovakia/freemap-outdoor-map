use crate::render::{
    Feature,
    ctx::Ctx,
    layer_render_error::LayerRenderResult,
    layers::{bridge_areas, contours, hillshading, hillshading_datasets::HillshadingDatasets},
};
use cairo::{Context, Format, ImageSurface, SurfacePattern};
use std::collections::HashMap;

const HILLSHADING_HIERARCHY: [(&str, &[&str]); 9] = [
    ("at", &["sk", "si", "cz"]),
    ("it", &["at", "ch", "si", "fr"]),
    ("ch", &["at", "fr"]),
    ("si", &[]),
    ("cz", &["sk", "pl"]),
    ("pl", &["sk"]),
    ("sk", &[]),
    ("fr", &[]),
    ("no", &[]),
];

pub fn render(
    ctx: &Ctx,
    context: &Context,
    bridge_rows: Vec<Feature>,
    mut contour_rows: HashMap<Option<&'static str>, Vec<Feature>>,
    hillshading_datasets: &mut HillshadingDatasets,
    do_shading: bool,
    do_contours: bool,
) -> LayerRenderResult {
    let _span = tracy_client::span!("shading_and_contours::render");

    let fade_alpha = 1.0f64.min(1.0 - (ctx.zoom as f64 - 7.0).ln() / 5.0);

    // Load all country mask surfaces once; reused by hillshading and contours.
    let mut country_masks: Vec<(&'static str, Option<ImageSurface>)> = HILLSHADING_HIERARCHY
        .iter()
        .map(|&(country, _)| {
            Ok((
                country,
                hillshading::load_surface(
                    ctx,
                    country,
                    hillshading_datasets,
                    hillshading::Mode::Mask,
                )?,
            ))
        })
        .collect::<Result<_, crate::render::layer_render_error::LayerRenderError>>()?;

    let tile_covered = {
        let mut present_mut: Vec<&mut ImageSurface> = country_masks
            .iter_mut()
            .filter_map(|(_, s)| s.as_mut())
            .collect();

        hillshading::mask_covers_tile(&mut present_mut)?
    };

    if ctx.zoom >= 15 {
        bridge_areas::render(ctx, context, bridge_rows, true)?; // mask
    }

    // CC = (mask, (contours-$cc, final-$cc):src-in, mask-$cut1:dst-out, mask-$cut2:dst-out, ...):src-over

    // (CC, CC, CC, (mask-$cc, mask-$cc, mask-$cc, (fallback_contours, fallback_final):src-out):src-over)

    if do_shading {
        for (country, better_countries) in HILLSHADING_HIERARCHY {
            let Some((_, Some(mask_surface))) = country_masks.iter().find(|(c, _)| *c == country)
            else {
                continue;
            };

            let Some(shading_surface) = hillshading::load_surface(
                ctx,
                country,
                hillshading_datasets,
                hillshading::Mode::Shading,
            )?
            else {
                continue;
            };

            context.push_group(); // country-contours-and-shading

            hillshading::paint_surface(ctx, context, mask_surface, 1.0)?;

            context.set_operator(cairo::Operator::In);
            hillshading::paint_surface(ctx, context, &shading_surface, fade_alpha)?;

            for better_country in better_countries {
                if let Some((_, Some(bc_mask))) =
                    country_masks.iter().find(|(c, _)| *c == *better_country)
                {
                    context.set_operator(cairo::Operator::DestOut);
                    hillshading::paint_surface(ctx, context, bc_mask, 1.0)?;
                }
            }

            context.pop_group_to_source()?; // country-contours-and-shading
            context.paint()?;
        }

        // fallback
        if !tile_covered {
            context.push_group(); // mask

            for (_, s) in &country_masks {
                if let Some(mask_surface) = s {
                    hillshading::paint_surface(ctx, context, mask_surface, 1.0)?;
                }
            }

            context.set_operator(cairo::Operator::Out);

            if let Some(surface) = hillshading::load_surface(
                ctx,
                "_",
                hillshading_datasets,
                hillshading::Mode::Shading,
            )? {
                hillshading::paint_surface(ctx, context, &surface, fade_alpha)?;
            }

            context.pop_group_to_source()?; // mask
            context.paint()?;
        }
    }

    if do_contours && ctx.zoom >= 12 {
        let scaled_w = (ctx.size.width as f64 * ctx.scale) as i32;
        let scaled_h = (ctx.size.height as f64 * ctx.scale) as i32;

        context.push_group(); // all contours — composited at 0.33 opacity via OVER

        // Per-country: render contours masked to (country mask − better-priority masks).
        for (country, better_countries) in HILLSHADING_HIERARCHY {
            let Some(rows) = contour_rows.remove(&Some(country)) else {
                continue;
            };

            if rows.is_empty() {
                continue;
            }

            let Some((_, Some(mask_surface))) = country_masks.iter().find(|(c, _)| *c == country)
            else {
                continue;
            };

            // Build combined mask on a CPU ImageSurface (DestOut is fine here — not on SVG context).
            let combined = ImageSurface::create(Format::ARgb32, scaled_w, scaled_h)?;
            {
                let cc = Context::new(&combined)?;
                cc.set_source_surface(mask_surface, 0.0, 0.0)?;
                cc.paint()?;
                cc.set_operator(cairo::Operator::DestOut);
                for bc in better_countries.iter() {
                    if let Some((_, Some(bc_mask))) = country_masks.iter().find(|(c, _)| *c == *bc)
                    {
                        cc.set_source_surface(bc_mask, 0.0, 0.0)?;
                        cc.paint()?;
                    }
                }
            }

            let mask_pattern = SurfacePattern::create(&combined);
            if ctx.scale != 1.0 {
                mask_pattern
                    .set_matrix(cairo::Matrix::new(ctx.scale, 0.0, 0.0, ctx.scale, 0.0, 0.0));
            }

            context.push_group();
            contours::render(ctx, context, rows)?;
            context.pop_group_to_source()?;
            context.mask(&mask_pattern)?;
        }

        // Fallback: render contours outside all known country masks.
        // Skipped when tile_covered: the complement would be fully transparent.
        if !tile_covered
            && let Some(rows) = contour_rows.remove(&None)
            && !rows.is_empty()
        {
            let complement = ImageSurface::create(Format::ARgb32, scaled_w, scaled_h)?;
            {
                let cc = Context::new(&complement)?;
                cc.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                cc.paint()?;
                cc.set_operator(cairo::Operator::DestOut);
                for (_, s) in &country_masks {
                    if let Some(mask_surface) = s {
                        cc.set_source_surface(mask_surface, 0.0, 0.0)?;
                        cc.paint()?;
                    }
                }
            }

            let mask_pattern = SurfacePattern::create(&complement);
            if ctx.scale != 1.0 {
                mask_pattern
                    .set_matrix(cairo::Matrix::new(ctx.scale, 0.0, 0.0, ctx.scale, 0.0, 0.0));
            }

            context.push_group();
            contours::render(ctx, context, rows)?;
            context.pop_group_to_source()?;
            context.mask(&mask_pattern)?;
        }

        context.pop_group_to_source()?;
        context.paint_with_alpha(0.33)?;
    }

    Ok(())
}
