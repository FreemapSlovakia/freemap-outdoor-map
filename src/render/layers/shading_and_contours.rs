use crate::render::{
    ContourCountries, Feature, HillshadingHierarchy,
    ctx::Ctx,
    layer_render_error::LayerRenderResult,
    layers::{bridge_areas, contours, hillshading, hillshading_datasets::HillshadingDatasets},
};
use cairo::{Context, Format, ImageSurface, SurfacePattern};
use std::collections::{HashMap, HashSet};

/// Hillshading / contour data sources and toggles for [`render`].
pub struct ShadingParams<'a> {
    pub datasets: &'a mut HillshadingDatasets,
    pub hierarchy: &'a HillshadingHierarchy,
    pub contour_countries: Option<&'a ContourCountries>,
    pub do_shading: bool,
}

pub fn render(
    ctx: &Ctx,
    context: &Context,
    bridge_rows: Vec<Feature>,
    mut contour_rows: HashMap<Option<&'static str>, Vec<Feature>>,
    params: ShadingParams,
) -> LayerRenderResult {
    let _span = tracy_client::span!("shading_and_contours::render");

    let ShadingParams {
        datasets: hillshading_datasets,
        hierarchy,
        contour_countries,
        do_shading,
    } = params;

    let fade_alpha = 1.0f64.min(1.0 - (ctx.zoom as f64 - 7.0).ln() / 5.0);

    // Load all country mask surfaces once; reused by hillshading and contours.
    let mut country_masks: Vec<(&'static str, Option<ImageSurface>)> = hierarchy
        .entries()
        .iter()
        .map(|entry| {
            Ok((
                entry.country,
                hillshading::load_surface(
                    ctx,
                    entry.country,
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
        for entry in hierarchy.entries() {
            let country = entry.country;

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

            for &better_country in &entry.better {
                if let Some((_, Some(bc_mask))) =
                    country_masks.iter().find(|(c, _)| *c == better_country)
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

    if ctx.zoom >= 12
        && let Some(contour_countries) = contour_countries
    {
        let scaled_w = (ctx.size.width as f64 * ctx.scale) as i32;
        let scaled_h = (ctx.size.height as f64 * ctx.scale) as i32;

        context.push_group(); // all contours — composited at 0.33 opacity via OVER

        // Countries that have a country-specific contour source. Countries with hillshading
        // only (e.g. "fi") are not in this set, so Norway's contours extend into Finland
        // unmasked rather than being cut at the Finnish border.
        let countries_with_contour_data: HashSet<&str> = contour_countries
            .entries()
            .iter()
            .map(|e| e.country)
            .collect();

        // Per-country: render contours masked to (country mask − better-priority masks that
        // also have contour data). A hillshading-only better country does not cut contours.
        for entry in hierarchy.entries() {
            let country = entry.country;

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
                for &bc in &entry.better {
                    if countries_with_contour_data.contains(bc)
                        && let Some((_, Some(bc_mask))) =
                            country_masks.iter().find(|(c, _)| *c == bc)
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

        // Fallback: render contours outside the masks of countries that have contour data.
        // Countries with hillshading only (e.g. "fi") are intentionally excluded — their
        // neighbouring country's contours already cover their area (see above).
        let contour_covered = {
            let mut present_mut: Vec<&mut ImageSurface> = country_masks
                .iter_mut()
                .filter(|(c, _)| countries_with_contour_data.contains(c))
                .filter_map(|(_, s)| s.as_mut())
                .collect();
            hillshading::mask_covers_tile(&mut present_mut)?
        };

        if !contour_covered
            && let Some(rows) = contour_rows.remove(&None)
            && !rows.is_empty()
        {
            let complement = ImageSurface::create(Format::ARgb32, scaled_w, scaled_h)?;
            {
                let cc = Context::new(&complement)?;
                cc.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                cc.paint()?;
                cc.set_operator(cairo::Operator::DestOut);
                for (country, s) in &country_masks {
                    if countries_with_contour_data.contains(country)
                        && let Some(mask_surface) = s
                    {
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
