use crate::render::RenderLayer;
use crate::render::{
    ImageFormat, collision::Collision, ctx::Ctx, layer_render_error::LayerRenderError, layers,
    layers::hillshading_datasets::HillshadingDatasets, projectable::TileProjector,
    render_request::RenderRequest, size::Size, svg_repo::SvgRepo,
};
use cairo::{Context, Surface};
use geo::Geometry;
use geo::Rect;
use postgres::Client;
use thiserror::Error;

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

impl From<cairo::Error> for RenderError {
    fn from(value: cairo::Error) -> Self {
        value.into()
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
    mut hillshading_datasets: Option<&mut HillshadingDatasets>,
    coverage_geometry: Option<&Geometry>,
    scale: f64,
) -> Result<(), RenderError> {
    let _span = tracy_client::span!("render_tile::draw");

    let context = &Context::new(surface)?;

    if scale != 1.0 {
        context.scale(scale, scale);
    }

    let collision = &mut Collision::new(Some(context));

    let zoom = request.zoom;

    let ctx = &Ctx {
        context,
        bbox,
        size,
        zoom,
        tile_projector: TileProjector::new(bbox, size),
        scale,
        legend: request.legend.as_ref(),
    };

    let coverage_geometry = if ctx.legend.is_none()
        && matches!(request.format, ImageFormat::Jpeg | ImageFormat::Png)
        && let Some(coverage_geometry) = coverage_geometry
    {
        context.set_source_rgb(0.82, 0.80, 0.78);
        context.paint().unwrap();

        ctx.context.push_group();

        Some(coverage_geometry)
    } else {
        None
    };

    if request.legend.is_none() {
        layers::sea::render(ctx, client).with_layer("sea")?;
    }

    // osm_landcovers (landcovers)
    layers::landcover::render(ctx, client, svg_repo).with_layer("landcover")?;

    let feature_line_rows = if zoom >= 11 {
        // osm_feature_lines (feature_lines)
        layers::feature_lines::query(ctx, client).map_err(|err| RenderError {
            layer: "feature_lines_query",
            source: err.into(),
        })?
    } else {
        vec![]
    };

    if zoom >= 13 {
        layers::feature_lines::render(ctx, 1, &feature_line_rows, svg_repo, None)
            .with_layer("feature_lines 1")?;
    }

    // waterways
    layers::water_lines::render(ctx, client, svg_repo).with_layer("water_lines")?;

    // waterareas
    layers::water_areas::render(ctx, client).with_layer("water_areas")?;

    if zoom >= 15 {
        // osm_landcovers (bridge_areas)
        layers::bridge_areas::render(ctx, client, false).with_layer("bridge_areas")?;
    }

    if zoom >= 16 {
        // pois
        layers::trees::render(ctx, client, svg_repo).with_layer("trees")?;
    }

    if zoom >= 12 {
        // feature_lines
        layers::feature_lines::render(
            ctx,
            2,
            &feature_line_rows,
            svg_repo,
            if request.render.contains(&RenderLayer::Shading) {
                hillshading_datasets.as_deref_mut()
            } else {
                None
            },
        )
        .with_layer("feature_lines 2")?;
    }

    if zoom >= 16 {
        // roads
        layers::embankments::render(ctx, client, svg_repo).with_layer("embankments")?;
    }

    if zoom >= 8 {
        // roads
        layers::roads::render(ctx, client, svg_repo).with_layer("roads")?;
    }

    if zoom >= 14 {
        // roads
        layers::road_access_restrictions::render(ctx, client, svg_repo)
            .with_layer("road_access_restrictions")?;
    }

    if zoom >= 11 {
        // feature_lines
        layers::feature_lines::render(ctx, 3, &feature_line_rows, svg_repo, None)
            .with_layer("feature_lines 3")?;
    }

    if (request.render.contains(&RenderLayer::Shading)
        || request.render.contains(&RenderLayer::Contours))
        && let Some(hillshading_datasets) = hillshading_datasets.as_deref_mut()
    {
        layers::shading_and_contours::render(
            ctx,
            client,
            hillshading_datasets,
            request.render.contains(&RenderLayer::Shading),
            request.render.contains(&RenderLayer::Contours),
        )
        .with_layer("shading_and_contours")?;
    }

    if zoom >= 12 {
        // osm_power_generators (solar_power_plants)
        layers::solar_power_plants::render(ctx, client).with_layer("solar_power_plants")?;
    }

    if zoom >= 13 {
        // osm_buildings (buildings)
        layers::buildings::render(ctx, client).with_layer("buildings")?;
    }

    if zoom >= 12 {
        // feature_lines
        layers::feature_lines::render(ctx, 4, &feature_line_rows, svg_repo, None)
            .with_layer("feature_lines 4")?;
    }

    if zoom >= 14 {
        // osm_pois (power_poles)
        layers::power_towers_poles::render(ctx, client).with_layer("power_towers_poles")?;
    }

    if zoom >= 8 {
        // osm_landcovers (protected_areas)
        layers::protected_areas::render(ctx, client, svg_repo).with_layer("protected_areas")?;
    }

    if zoom >= 13 {
        // osm_landcovers (special_parks)
        layers::special_parks::render(ctx, client).with_layer("special_parks")?;
    }

    if zoom >= 10 {
        // osm_landcovers (military_areas)
        layers::military_areas::render(ctx, client).with_layer("military_areas")?;
    }

    if zoom >= 8 && request.render.contains(&RenderLayer::CountryBorders) {
        // osm_country_members (country_borders)
        layers::borders::render(ctx, client).with_layer("borders")?;
    }

    if zoom
        >= if request.render.contains(&RenderLayer::RoutesHikingKst) {
            8
        } else {
            9
        }
    {
        // osm_routes, osm_route_members (routes)
        layers::routes::render_marking(ctx, client, &request.render, svg_repo)
            .with_layer("routes")?;
    }

    if (9..=11).contains(&zoom) && request.render.contains(&RenderLayer::Geonames) {
        // geonames_smooth (geonames)
        layers::geonames::render(ctx, client).with_layer("geonames")?;
    }

    if (8..=14).contains(&zoom) {
        // osm_places (place_names_lowzoom)
        layers::place_names::render(ctx, client, &mut Some(collision)).with_layer("place_names")?;
    }

    if (8..=10).contains(&zoom) {
        // osm_landcovers (national_park_names)
        layers::national_park_names::render(ctx, client, collision)
            .with_layer("national_park_names")?;
    }

    if (13..=16).contains(&zoom) {
        // osm_pois (special_park_names)
        layers::special_park_names::render(ctx, client, collision)
            .with_layer("special_park_names")?;
    }

    if zoom >= 10 {
        // osm_pois (pois)
        layers::pois::render(
            ctx,
            client,
            collision,
            svg_repo,
            request.render.contains(&RenderLayer::RoutesHikingKst),
        )
        .with_layer("features")?;
    }

    if zoom >= 10 {
        // osm_waterareas (water_area_names)
        layers::water_area_names::render(ctx, client, collision).with_layer("water_area_names")?;
    }

    if zoom >= 17 {
        // osm_buildings (building_names)
        layers::building_names::render(ctx, client, collision).with_layer("building_names")?;
    }

    if zoom >= 12 {
        // osm_landcovers (landcovers)
        layers::bordered_area_names::render(ctx, client, collision)
            .with_layer("protected_area_names")?;
    }

    if zoom >= 12 {
        // osm_landcovers (landcovers)
        layers::landcover_names::render(ctx, client, collision).with_layer("landcover_names")?;
    }

    if zoom >= 15 {
        // osm_places (locality_names)
        layers::locality_names::render(ctx, client, collision).with_layer("locality_names")?;
    }

    if zoom >= 18 {
        // osm_housenumbers (housenumbers)
        layers::housenumbers::render(ctx, client, collision).with_layer("housenumbers")?;
    }

    if zoom >= 15 {
        // osm_roads (roads)
        layers::highway_names::render(ctx, client, collision).with_layer("highway_names")?;
    }

    if zoom >= 14 {
        // osm_routes, osm_route_members (routes)
        layers::routes::render_labels(ctx, client, &request.render, collision)
            .with_layer("routes")?;
    }

    if zoom >= 16 {
        // osm_feature_lines (feature_lines)
        layers::aerialway_names::render(ctx, client, collision).with_layer("aerialway_names")?;
    }

    if zoom >= 12 {
        // water_lines (osm_waterways)
        layers::water_line_names::render(ctx, client, collision).with_layer("water_line_names")?;
    }

    if zoom >= 14 {
        // osm_fixmes (fixmes)
        layers::fixmes::render(ctx, client, svg_repo).with_layer("fixmes")?;
    }

    if zoom >= 13 {
        // osm_feature_lines (valleys_ridges)
        layers::valleys_ridges::render(ctx, client).with_layer("valleys_ridges")?;
    }

    if (15..=17).contains(&zoom) {
        // osm_places (place_names)
        layers::place_names::render(ctx, client, &mut Some(collision))
            .with_layer("place_names_highzoom")?;
    }

    if zoom < 8 && request.render.contains(&RenderLayer::CountryNames) {
        // country_names_smooth (country_names)
        layers::country_names::render(ctx, client).with_layer("country_names")?;
    }

    if let Some(coverage_geometry) = coverage_geometry {
        layers::blur_edges::render(ctx, coverage_geometry).with_layer("blur_edges")?;
        ctx.context.pop_group_to_source()?;
        ctx.context.paint()?;
    }

    if let Some(ref features) = request.featues {
        layers::custom::render(ctx, features).with_layer("custom")?;
    }

    if let Some(hillshading_datasets) = hillshading_datasets {
        hillshading_datasets.evict_unused();
    }

    Ok(())
}
