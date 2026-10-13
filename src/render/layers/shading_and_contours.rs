use crate::render::{
    ContourCountries, Feature, HillshadingHierarchy,
    attribution::{Attribution, FALLBACK_KEY},
    ctx::Ctx,
    draw::mask,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::{
        bridge_areas, contours, dry_land::DryLand, hillshading, hillshading::MaskBits,
        hillshading_datasets::HillshadingDatasets,
    },
};
use cairo::{Context, ImageSurface};
use std::collections::{HashMap, HashSet};

/// Hillshading / contour data sources and toggles for [`render`].
pub struct ShadingParams<'a> {
    pub datasets: &'a HillshadingDatasets,
    pub hierarchy: &'a HillshadingHierarchy,
    pub contour_countries: Option<&'a ContourCountries>,
    pub do_shading: bool,
    pub dry_land: Option<&'a DryLand>,
    /// Collects the code of every dataset that ends up with a pixel on the tile.
    pub attribution: &'a mut Attribution,
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
        dry_land,
        attribution,
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
        .collect::<Result<_, LayerRenderError>>()?;

    // The masks are already resampled to tile resolution, so these bits live in the
    // same coordinate space as the pixels being credited: a dataset contributes
    // exactly when a bit of its mask survives the `DestOut` subtraction below.
    let mask_bits: HashMap<&'static str, MaskBits> = country_masks
        .iter_mut()
        .filter_map(|(country, surface)| {
            let surface = surface.as_mut()?;

            Some(MaskBits::from_surface(surface).map(|bits| (*country, bits)))
        })
        .collect::<Result<_, LayerRenderError>>()?;

    let tile_covered = MaskBits::union(mask_bits.values()).is_some_and(|union| union.is_full());

    if ctx.zoom >= 15 {
        bridge_areas::render(ctx, context, bridge_rows, true)?; // mask
    }

    // CC = (mask, (contours-$cc, final-$cc):src-in, mask-$cut1:dst-out, mask-$cut2:dst-out, ...):src-over

    // (CC, CC, CC, (mask-$cc, mask-$cc, mask-$cc, (fallback_contours, fallback_final):src-out):src-over)

    if do_shading {
        // Merged first, so a PDF embeds one shading image rather than one per country.
        let mut shading: Option<ImageSurface> = None;

        for entry in hierarchy.entries() {
            let country = entry.country;

            let Some((_, Some(mask_surface))) = country_masks.iter().find(|(c, _)| *c == country)
            else {
                continue;
            };

            let Some(mut shading_surface) = hillshading::load_surface(
                ctx,
                country,
                hillshading_datasets,
                hillshading::Mode::Shading,
            )?
            else {
                continue;
            };

            let better: Vec<&MaskBits> = entry
                .better
                .iter()
                .filter_map(|better| mask_bits.get(better))
                .collect();

            // Against the shading's own alpha as well as the mask, the same test the
            // fallback below makes: having data somewhere in the tile is not the same
            // as having drawn anything here. The one thing this cannot see is the
            // dry-land clip `pipeline` wraps the whole layer in, so a dataset whose
            // only pixels on this tile fall on water is still credited.
            if let Some(bits) = mask_bits.get(country) {
                let painted = MaskBits::from_surface(&mut shading_surface)?;

                if bits.has_any_shared_outside(&painted, &better) {
                    attribution.add_shading(country);
                }
            }

            let composite = mask::compose(ctx, |cc| {
                mask::paint_whole(cc, mask_surface, 1.0)?;
                cc.set_operator(cairo::Operator::In);
                mask::paint_whole(cc, &shading_surface, fade_alpha)?;
                cc.set_operator(cairo::Operator::DestOut);
                for &better_country in &entry.better {
                    if let Some((_, Some(bc_mask))) =
                        country_masks.iter().find(|(c, _)| *c == better_country)
                    {
                        mask::paint_whole(cc, bc_mask, 1.0)?;
                    }
                }
                Ok(())
            })?;

            merge(&mut shading, composite)?;
        }

        // fallback
        if !tile_covered
            && let Some(mut surface) = hillshading::load_surface(
                ctx,
                FALLBACK_KEY,
                hillshading_datasets,
                hillshading::Mode::Shading,
            )?
        {
            // The fallback is what is left after `Out`, so its own alpha decides:
            // a tile the country masks leave uncovered may still have no fallback
            // data there.
            let covered: Vec<&MaskBits> = mask_bits.values().collect();

            if MaskBits::from_surface(&mut surface)?.has_any_outside(&covered) {
                attribution.add_shading(FALLBACK_KEY);
            }

            let composite = mask::compose(ctx, |cc| {
                for mask_surface in country_masks.iter().filter_map(|(_, s)| s.as_ref()) {
                    mask::paint_whole(cc, mask_surface, 1.0)?;
                }
                cc.set_operator(cairo::Operator::Out);
                mask::paint_whole(cc, &surface, fade_alpha)
            })?;

            merge(&mut shading, composite)?;
        }

        if let Some(shading) = shading {
            hillshading::paint_surface(ctx, context, &shading, 1.0)?;
        }
    }

    if ctx.zoom >= 12
        && let Some(contour_countries) = contour_countries
    {
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

            let cutting: Vec<&MaskBits> = entry
                .better
                .iter()
                .filter(|better| countries_with_contour_data.contains(*better))
                .filter_map(|better| mask_bits.get(better))
                .collect();

            // The region this country's contours may draw in is non-empty and it has
            // rows; whether a line actually falls inside it is not checked, so a tile
            // whose rows all miss the region is over-credited.
            if mask_bits
                .get(country)
                .is_some_and(|bits| bits.has_any_outside(&cutting))
            {
                attribution.add_contours(country);
            }

            let combined = mask::compose(ctx, |cc| {
                mask::paint_whole(cc, mask_surface, 1.0)?;
                cc.set_operator(cairo::Operator::DestOut);
                for &bc in &entry.better {
                    if countries_with_contour_data.contains(bc)
                        && let Some((_, Some(bc_mask))) =
                            country_masks.iter().find(|(c, _)| *c == bc)
                    {
                        mask::paint_whole(cc, bc_mask, 1.0)?;
                    }
                }
                Ok(())
            })?;

            context.push_group();
            contours::render(ctx, context, rows, dry_land)?;
            context.pop_group_to_source()?;
            mask::mask_with(ctx, context, &combined)?;
        }

        // Fallback: render contours outside the masks of countries that have contour data.
        // Countries with hillshading only (e.g. "fi") are intentionally excluded — their
        // neighbouring country's contours already cover their area (see above).
        let contour_covered = MaskBits::union(
            mask_bits
                .iter()
                .filter(|(country, _)| countries_with_contour_data.contains(*country))
                .map(|(_, bits)| bits),
        )
        .is_some_and(|union| union.is_full());

        if !contour_covered
            && let Some(rows) = contour_rows.remove(&None)
            && !rows.is_empty()
        {
            attribution.add_contours(FALLBACK_KEY);

            let complement = mask::compose(ctx, |cc| {
                cc.paint()?;
                cc.set_operator(cairo::Operator::DestOut);
                for (country, s) in &country_masks {
                    if countries_with_contour_data.contains(country)
                        && let Some(mask_surface) = s
                    {
                        mask::paint_whole(cc, mask_surface, 1.0)?;
                    }
                }
                Ok(())
            })?;

            context.push_group();
            contours::render(ctx, context, rows, dry_land)?;
            context.pop_group_to_source()?;
            mask::mask_with(ctx, context, &complement)?;
        }

        context.pop_group_to_source()?;
        context.paint_with_alpha(0.33)?;
    }

    Ok(())
}

fn merge(acc: &mut Option<ImageSurface>, surface: ImageSurface) -> LayerRenderResult {
    if let Some(acc) = acc.as_ref() {
        return mask::paint_whole(&Context::new(acc)?, &surface, 1.0);
    }

    *acc = Some(surface);

    Ok(())
}
