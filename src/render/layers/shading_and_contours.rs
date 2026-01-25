use crate::render::{
    ctx::Ctx,
    layer_render_error::LayerRenderResult,
    layers::{bridge_areas, contours, hillshading, hillshading_datasets::HillshadingDatasets},
};
use postgres::Client;

const FALLBACK: bool = true;

pub fn render(
    ctx: &Ctx,
    client: &mut Client,
    hillshading_datasets: &mut HillshadingDatasets,
    shading: bool,
    contours: bool,
) -> LayerRenderResult {
    let _span = tracy_client::span!("shading_and_contours::render");

    let fade_alpha = 1.0f64.min(1.0 - (ctx.zoom as f64 - 7.0).ln() / 5.0);

    let context = ctx.context;

    context.push_group(); // top

    if ctx.zoom >= 15 {
        bridge_areas::render(ctx, client, true)?; // mask
    }

    // CC = (mask, (contours-$cc, final-$cc):src-in, mask-$cut1:dst-out, mask-$cut2:dst-out, ...):src-over

    // (CC, CC, CC, (mask-$cc, mask-$cc, mask-$cc, (fallback_contours, fallback_final):src-out):src-over)

    let config: Vec<(&str, Vec<&str>)> = vec![
        ("at", vec!["sk", "si", "cz"]),
        ("it", vec!["at", "ch", "si", "fr"]),
        ("ch", vec!["at", "fr"]),
        ("si", vec![]),
        ("cz", vec!["sk", "pl"]),
        ("pl", vec!["sk"]),
        ("sk", vec![]),
        ("fr", vec![]),
        ("no", vec![]),
    ];

    for (country, better_countries) in config {
        let mask_surface =
            hillshading::load_surface(ctx, country, hillshading_datasets, hillshading::Mode::Mask)?;

        let Some(mask_surface) = mask_surface else {
            continue;
        };

        context.push_group(); // country-contours-and-shading

        hillshading::paint_surface(ctx, &mask_surface, 1.0)?;

        context.push_group(); // contours-and-shading

        if contours && ctx.zoom >= 12 {
            context.push_group(); // contours
            contours::render(ctx, client, Some(country))?;
            context.pop_group_to_source()?; // contours
            context.paint_with_alpha(0.33)?;
        }

        if shading {
            hillshading::load_and_paint(
                ctx,
                country,
                fade_alpha,
                hillshading_datasets,
                hillshading::Mode::Shading,
            )?;
        }

        context.pop_group_to_source()?; // contours-and-shading
        context.set_operator(cairo::Operator::In);
        context.paint()?;

        if shading {
            for better_country in better_countries {
                context.set_operator(cairo::Operator::DestOut);

                hillshading::load_and_paint(
                    ctx,
                    better_country,
                    1.0,
                    hillshading_datasets,
                    hillshading::Mode::Mask,
                )?;
            }
        }

        context.pop_group_to_source()?; // // country-contours-and-shading
        context.paint()?;
    }

    if FALLBACK {
        let mut mask_surfaces = Vec::new();

        for country in ["it", "at", "ch", "si", "pl", "sk", "cz", "fr", "no"] {
            let mask_surface = hillshading::load_surface(
                ctx,
                country,
                hillshading_datasets,
                hillshading::Mode::Mask,
            )?;

            if let Some(mask_surface) = mask_surface {
                mask_surfaces.push(mask_surface);
            }
        }

        if !hillshading::mask_covers_tile(&mut mask_surfaces)? {
            context.push_group(); // mask

            for mask_surface in &mask_surfaces {
                hillshading::paint_surface(ctx, mask_surface, 1.0)?;
            }

            context.push_group(); // fallback

            {
                let _span = tracy_client::span!("shading_and_contours::contours");

                if contours && ctx.zoom >= 12 {
                    context.push_group(); // contours
                    contours::render(ctx, client, None)?;
                    context.pop_group_to_source()?; // contours
                    context.paint_with_alpha(0.33)?;
                }
            }

            if shading {
                hillshading::load_and_paint(
                    ctx,
                    "_",
                    fade_alpha,
                    hillshading_datasets,
                    hillshading::Mode::Shading,
                )?;
            }

            context.pop_group_to_source()?; // fallback
            context.set_operator(cairo::Operator::Out);
            context.paint()?;

            context.pop_group_to_source()?; // mask
            context.paint()?;
        }
    }

    context.pop_group_to_source()?; // top
    context.paint()?;

    Ok(())
}
