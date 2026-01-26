use crate::render::{
    ImageFormat, SvgRepo, collision::Collision, ctx::Ctx, layer_render_error::LayerRenderError,
    layers::hillshading_datasets::HillshadingDatasets, projectable::TileProjector,
    render_request::RenderRequest, size::Size,
};
use cairo::{Context, Surface};
use geo::Geometry;
use geo::Rect;
use postgres::Client;
use thiserror::Error;

mod aerialway_names;
mod aerialways;
mod aeroways;
mod barrierways;
mod blur_edges;
mod borders;
mod bridge_areas;
mod building_names;
mod buildings;
mod contours;
mod country_names;
mod custom;
mod cutlines;
mod embankments;
mod feature_lines;
mod feature_lines_maskable;
mod feature_z_order;
mod features;
mod fixmes;
mod geonames;
mod highway_names;
mod hillshading;
pub(crate) mod hillshading_datasets;
mod housenumbers;
mod landcover;
mod landcover_names;
mod landcover_z_order;
mod locality_names;
mod military_areas;
mod national_park_names;
mod pipelines;
mod place_names;
mod power_lines;
mod protected_area_names;
mod protected_areas;
mod road_access_restrictions;
mod roads;
pub(crate) mod routes;
mod sea;
mod shading_and_contours;
mod solar_power_plants;
mod special_park_names;
mod special_parks;
mod trees;
mod valleys_ridges;
mod water_area_names;
mod water_areas;
mod water_line_names;
mod water_lines;

#[derive(Error, Debug)]
#[error("Failed to render \"{layer}\": {source}")]
pub struct RenderError {
    pub layer: &'static str,

    #[source]
    pub source: LayerRenderError,
}

impl RenderError {
    pub fn new(layer: &'static str, source: LayerRenderError) -> Self {
        Self { layer, source }
    }
}

pub trait WithLayer<T> {
    fn with_layer(self, layer: &'static str) -> Result<T, RenderError>;
}

impl<T> WithLayer<T> for Result<T, LayerRenderError> {
    fn with_layer(self, layer: &'static str) -> Result<T, RenderError> {
        self.map_err(|err| RenderError::new(layer, err))
    }
}

pub fn render(
    surface: &Surface,
    request: &RenderRequest,
    client: &mut Client,
    bbox: Rect<f64>,
    size: Size<u32>,
    svg_repo: &mut SvgRepo,
    hillshading_datasets: &mut Option<HillshadingDatasets>,
    mask_geometry: Option<&Geometry>,
    render_scale: f64,
) -> Result<(), RenderError> {
    let _span = tracy_client::span!("render_tile::draw");

    let context = &Context::new(surface)
        .map_err(LayerRenderError::from)
        .with_layer("top")?;

    if render_scale != 1.0 {
        context.scale(render_scale, render_scale);
    }

    let collision = &mut Collision::new(Some(context));

    let zoom = request.zoom;

    let ctx = &Ctx {
        context,
        bbox,
        size,
        zoom,
        tile_projector: TileProjector::new(bbox, size),
        scale: render_scale,
    };

    sea::render(ctx, client).with_layer("sea")?;

    ctx.context.push_group();

    landcover::render(ctx, client, svg_repo).with_layer("landuse")?;

    if zoom >= 13 {
        cutlines::render(ctx, client).with_layer("cutlines")?;
    }

    water_lines::render(ctx, client, svg_repo).with_layer("water_lines")?;

    water_areas::render(ctx, client).with_layer("water_areas")?;

    if zoom >= 15 {
        bridge_areas::render(ctx, client, false).with_layer("bridge_areas")?;
    }

    if zoom >= 16 {
        trees::render(ctx, client, svg_repo).with_layer("trees")?;
    }

    if zoom >= 12 {
        pipelines::render(ctx, client).with_layer("pipelines")?;
    }

    if zoom >= 13 {
        feature_lines::render(ctx, client, svg_repo).with_layer("feature_lines")?;
    }

    if zoom >= 15 {
        feature_lines_maskable::render(
            ctx,
            client,
            svg_repo,
            hillshading_datasets,
            request.shading,
        )
        .with_layer("feature_lines_maskable")?;
    }

    if zoom >= 16 {
        embankments::render(ctx, client, svg_repo).with_layer("embankments")?;
    }

    if zoom >= 8 {
        roads::render(ctx, client, svg_repo).with_layer("roads")?;
    }

    if zoom >= 14 {
        road_access_restrictions::render(ctx, client, svg_repo)
            .with_layer("road_access_restrictions")?;
    }

    if (request.shading || request.contours)
        && let Some(hillshading_datasets) = hillshading_datasets
    {
        shading_and_contours::render(
            ctx,
            client,
            hillshading_datasets,
            request.shading,
            request.contours,
        )
        .with_layer("shading_and_contours")?;
    }

    if zoom >= 11 {
        aeroways::render(ctx, client).with_layer("aeroways")?;
    }

    if zoom >= 12 {
        solar_power_plants::render(ctx, client).with_layer("solar_power_plants")?;
    }

    if zoom >= 13 {
        buildings::render(ctx, client).with_layer("buildings")?;
    }

    if zoom >= 16 {
        barrierways::render(ctx, client).with_layer("barrierways")?;
    }

    if zoom >= 12 {
        aerialways::render(ctx, client).with_layer("aerialways")?;
    }

    if zoom >= 13 {
        power_lines::render_lines(ctx, client).with_layer("power_lines")?;
    }

    if zoom >= 14 {
        power_lines::render_towers_poles(ctx, client).with_layer("power_lines")?;
    }

    if zoom >= 8 {
        protected_areas::render(ctx, client, svg_repo).with_layer("protected_areas")?;
    }

    if zoom >= 13 {
        special_parks::render(ctx, client).with_layer("special_parks")?;
    }

    if zoom >= 10 {
        military_areas::render(ctx, client).with_layer("military_areas")?;
    }

    if zoom >= 8 {
        borders::render(ctx, client).with_layer("borders")?;
    }

    routes::render_marking(ctx, client, &request.route_types, svg_repo).with_layer("routes")?;

    if (9..=11).contains(&zoom) {
        geonames::render(ctx, client).with_layer("geonames")?;
    }

    if (8..=14).contains(&zoom) {
        place_names::render(ctx, client, &mut Some(collision)).with_layer("place_names")?;
    }

    if (8..=10).contains(&zoom) {
        national_park_names::render(ctx, client, collision).with_layer("national_park_names")?;
    }

    if (13..=16).contains(&zoom) {
        special_park_names::render(ctx, client, collision).with_layer("special_park_names")?;
    }

    if zoom >= 10 {
        features::render(ctx, client, collision, svg_repo).with_layer("features")?;
    }

    if zoom >= 10 {
        water_area_names::render(ctx, client, collision).with_layer("water_area_names")?;
    }

    if zoom >= 17 {
        building_names::render(ctx, client, collision).with_layer("building_names")?;
    }

    if zoom >= 12 {
        protected_area_names::render(ctx, client, collision).with_layer("protected_area_names")?;
    }

    if zoom >= 12 {
        landcover_names::render(ctx, client, collision).with_layer("landcover_names")?;
    }

    if zoom >= 15 {
        locality_names::render(ctx, client, collision).with_layer("locality_names")?;
    }

    if zoom >= 18 {
        housenumbers::render(ctx, client, collision).with_layer("housenumbers")?;
    }

    if zoom >= 15 {
        highway_names::render(ctx, client, collision).with_layer("highway_names")?;
    }

    if zoom >= 14 {
        routes::render_labels(ctx, client, &request.route_types, collision).with_layer("routes")?;
    }

    if zoom >= 16 {
        aerialway_names::render(ctx, client, collision).with_layer("aerialway_names")?;
    }

    if zoom >= 12 {
        water_line_names::render(ctx, client, collision).with_layer("water_line_names")?;
    }

    if zoom >= 14 {
        fixmes::render(ctx, client, svg_repo).with_layer("fixmes")?;
    }

    if zoom >= 13 {
        valleys_ridges::render(ctx, client).with_layer("valleys_ridges")?;
    }

    if zoom >= 15 {
        place_names::render(ctx, client, &mut None).with_layer("place_names")?;
    }

    if matches!(request.format, ImageFormat::Jpeg | ImageFormat::Png) {
        blur_edges::render(ctx, mask_geometry).with_layer("blur_edges")?;
    }

    ctx.context
        .pop_group_to_source()
        .and_then(|_| ctx.context.paint())
        .map_err(LayerRenderError::from)
        .with_layer("top")?;

    if zoom < 8 {
        country_names::render(ctx, client).with_layer("country_names")?;
    }

    if let Some(ref features) = request.featues {
        custom::render(ctx, features).with_layer("custom")?;
    }

    if let Some(hillshading_datasets) = hillshading_datasets {
        hillshading_datasets.evict_unused();
    }

    Ok(())
}
