use crate::render::{
    ContourCountries, Feature, HillshadingHierarchy,
    attribution::{Attribution, FALLBACK_KEY},
    ctx::Ctx,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::{
        bridge_areas, contours, dry_land::DryLand, hillshading, hillshading::MaskBits,
        hillshading_datasets::HillshadingDatasets,
    },
};
use cairo::{Context, Format, ImageSurface, SurfacePattern};
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

            if let Some(mut surface) = hillshading::load_surface(
                ctx,
                FALLBACK_KEY,
                hillshading_datasets,
                hillshading::Mode::Shading,
            )? {
                // The fallback is what is left after `Out`, so its own alpha decides:
                // a tile the country masks leave uncovered may still have no fallback
                // data there.
                let covered: Vec<&MaskBits> = mask_bits.values().collect();

                if MaskBits::from_surface(&mut surface)?.has_any_outside(&covered) {
                    attribution.add_shading(FALLBACK_KEY);
                }

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
            #[allow(clippy::float_cmp)] // exact identity check: skip transform when scale is 1.0
            if ctx.scale != 1.0 {
                mask_pattern
                    .set_matrix(cairo::Matrix::new(ctx.scale, 0.0, 0.0, ctx.scale, 0.0, 0.0));
            }

            context.push_group();
            contours::render(ctx, context, rows, dry_land)?;
            context.pop_group_to_source()?;
            context.mask(&mask_pattern)?;
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
            #[allow(clippy::float_cmp)] // exact identity check: skip transform when scale is 1.0
            if ctx.scale != 1.0 {
                mask_pattern
                    .set_matrix(cairo::Matrix::new(ctx.scale, 0.0, 0.0, ctx.scale, 0.0, 0.0));
            }

            context.push_group();
            contours::render(ctx, context, rows, dry_land)?;
            context.pop_group_to_source()?;
            context.mask(&mask_pattern)?;
        }

        context.pop_group_to_source()?;
        context.paint_with_alpha(0.33)?;
    }

    Ok(())
}
