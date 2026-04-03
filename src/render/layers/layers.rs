use crate::render::colors::ContextExt;
use crate::render::projectable::TileProjectable;
use crate::render::{
    Feature, ImageFormat,
    collision::Collision,
    ctx::Ctx,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers,
    layers::hillshading_datasets::HillshadingDatasets,
    projectable::TileProjector,
    render_request::RenderRequest,
    size::Size,
    svg_repo::SvgRepo,
};
use crate::render::{RenderLayer, colors};
use cairo::{Context, Surface};
use geo::Rect;
use postgres::{Client, NoTls};
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use std::thread;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("Failed to render \"{layer}\": {source}")]
    Layer {
        layer: &'static str,
        #[source]
        source: LayerRenderError,
    },

    #[error("Cairo error: {0}")]
    Cairo(#[from] cairo::Error),

    #[error("Render thread panicked")]
    ThreadPanic,
}

impl RenderError {
    pub fn new(layer: &'static str, source: LayerRenderError) -> Self {
        Self::Layer { layer, source }
    }
}

trait WithLayer<T> {
    fn with_layer(self, layer: &'static str) -> Result<T, RenderError>;
}

impl<T> WithLayer<T> for Result<T, LayerRenderError> {
    fn with_layer(self, layer: &'static str) -> Result<T, RenderError> {
        self.map_err(|err| RenderError::new(layer, err))
    }
}

impl<T> WithLayer<T> for Result<T, postgres::Error> {
    fn with_layer(self, layer: &'static str) -> Result<T, RenderError> {
        self.map_err(|err| RenderError::new(layer, LayerRenderError::from(err)))
    }
}

enum QueryState {
    Pending(thread::JoinHandle<Result<Vec<Feature>, LayerRenderError>>),
    Ready(Arc<Vec<Feature>>),
}

#[derive(Clone)]
struct QueryHandle {
    name: &'static str,
    state: Arc<Mutex<QueryState>>,
}

impl QueryHandle {
    fn resolve(&self) -> Result<Arc<Vec<Feature>>, RenderError> {
        let mut guard = self.state.lock().unwrap();

        if let QueryState::Ready(rows) = guard.deref() {
            return Ok(rows.clone());
        }

        let QueryState::Pending(jh) =
            std::mem::replace(guard.deref_mut(), QueryState::Ready(Arc::new(vec![])))
        else {
            unreachable!()
        };

        let rows = Arc::new(
            jh.join()
                .map_err(|_| RenderError::ThreadPanic)?
                .with_layer(self.name)?,
        );

        *guard = QueryState::Ready(rows.clone());

        Ok(rows)
    }

    fn consume(self) -> Result<Vec<Feature>, RenderError> {
        let state = Arc::into_inner(self.state)
            .expect("sole owner of QueryHandle")
            .into_inner()
            .unwrap();
        match state {
            QueryState::Pending(jh) => jh
                .join()
                .map_err(|_| RenderError::ThreadPanic)?
                .with_layer(self.name),
            QueryState::Ready(arc) => {
                Ok(Arc::into_inner(arc).expect("sole Arc owner after consume"))
            }
        }
    }
}

type Pender<'a> = Box<dyn FnOnce(Params) -> Result<(), RenderError> + 'a>;

struct Prefetcher<'a> {
    pending: Vec<Pender<'a>>,
    pool: Pool<PostgresConnectionManager<NoTls>>,
    ctx: Arc<Ctx>,
}

struct Params<'p, 'ctx> {
    collision: &'p mut Collision<'ctx>,
    svg_repo: &'p mut SvgRepo,
    hsd: Option<&'p mut HillshadingDatasets>,
}

impl<'a> Prefetcher<'a> {
    fn new(pool: Pool<PostgresConnectionManager<NoTls>>, ctx: Arc<Ctx>) -> Self {
        Self {
            pending: Vec::new(),
            pool,
            ctx,
        }
    }

    fn prefetch(
        &self,
        name: &'static str,
        query_fn: impl FnOnce(&Ctx, &mut Client) -> Result<Vec<Feature>, LayerRenderError>
        + Send
        + 'static,
    ) -> QueryHandle {
        let pool = self.pool.clone();
        let ctx = self.ctx.clone();

        let jh = thread::spawn(move || {
            let mut conn = {
                let _span = tracy_client::span!("pool::get");

                pool.get()?
            };

            query_fn(&ctx, &mut conn)
        });

        QueryHandle {
            name,
            state: Arc::new(Mutex::new(QueryState::Pending(jh))),
        }
    }

    fn add(
        &mut self,
        name: &'static str,
        query_fn: impl FnOnce(&Ctx, &mut Client) -> Result<Vec<Feature>, LayerRenderError>
        + Send
        + 'static,
        render_fn: impl for<'p, 'ctx> FnOnce(Vec<Feature>, Params<'p, 'ctx>) -> LayerRenderResult + 'a,
    ) {
        let pool = self.pool.clone();
        let ctx = self.ctx.clone();

        let jh = thread::spawn(move || {
            let mut conn = {
                let _span = tracy_client::span!("pool::get");

                pool.get()?
            };

            query_fn(&ctx, &mut conn)
        });

        self.pending.push(Box::new(move |params| {
            let rows = jh
                .join()
                .map_err(|_| RenderError::ThreadPanic)?
                .with_layer(name)?;

            render_fn(rows, params).with_layer(name)
        }));
    }

    fn add_with(
        &mut self,
        handle: QueryHandle,
        render_fn: impl FnOnce(Arc<Vec<Feature>>, Params) -> LayerRenderResult + 'a,
    ) {
        self.pending.push(Box::new(move |params| {
            let rows = handle.resolve()?;
            render_fn(rows, params).with_layer(handle.name)
        }));
    }

    fn push(&mut self, render_fn: impl FnOnce(Params) -> Result<(), RenderError> + 'a) {
        self.pending.push(Box::new(move |params| render_fn(params)));
    }

    fn run(
        self,
        svg_repo: &mut SvgRepo,
        mut hsd: Option<&mut HillshadingDatasets>,
        collision: &mut Collision,
    ) -> Result<(), RenderError> {
        for layer in self.pending {
            layer(Params {
                svg_repo,
                hsd: hsd.as_deref_mut(),
                collision,
            })?;
        }
        Ok(())
    }
}

pub fn render(
    surface: &Surface,
    request: &RenderRequest,
    pool: Pool<PostgresConnectionManager<NoTls>>,
    bbox: Rect<f64>,
    size: Size<u32>,
    svg_repo: &mut SvgRepo,
    mut hillshading_datasets: Option<&mut HillshadingDatasets>,
) -> Result<(), RenderError> {
    let _span = tracy_client::span!("render_tile::draw");

    let context = &Context::new(surface)?;

    let scale = request.scale;

    if scale != 1.0 {
        context.scale(scale, scale);
    }

    let collision = &mut Collision::new(Some(context));

    let zoom = request.zoom;

    let to_render = &request.to_render;

    let do_shading = to_render.contains(&RenderLayer::Shading);
    let do_contours = to_render.contains(&RenderLayer::Contours);

    let legend = request.legend.clone();

    let ctx = Arc::new(Ctx {
        bbox,
        size,
        zoom,
        tile_projector: TileProjector::new(bbox, size),
        scale,
        legend,
    });

    let coverage_geometry = if ctx.legend.is_none()
        && matches!(request.format, ImageFormat::Jpeg | ImageFormat::Png)
        && let Some(ref coverage_geometry) = request.coverage_geometry
    {
        context.set_source_rgb(0.82, 0.80, 0.78);
        context.paint().unwrap();

        context.push_group();

        Some(coverage_geometry)
    } else {
        None
    };

    let mut prefetcher = Prefetcher::new(pool.clone(), ctx.clone());

    if request.legend.is_none() {
        prefetcher.add(
            "sea",
            |ctx, conn| layers::sea::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::sea::render(&ctx, context, rows),
        );
    }

    // osm_landcovers (landcovers)
    prefetcher.add(
        "landcover",
        |ctx, conn| layers::landcover::query(ctx, conn).map_err(Into::into),
        |rows, params| layers::landcover::render(&ctx, context, rows, params.svg_repo),
    );

    let feature_lines = (zoom >= 11).then(|| {
        prefetcher.prefetch("feature_lines", |ctx, conn| {
            layers::feature_lines::query(ctx, conn).map_err(Into::into)
        })
    });

    if zoom >= 13
        && let Some(handle) = feature_lines.clone()
    {
        prefetcher.add_with(handle, |rows, params| {
            layers::feature_lines::render(&ctx, context, 1, &rows, params.svg_repo, None)
        });
    }

    // waterways
    prefetcher.add(
        "water_lines",
        |ctx, conn| layers::water_lines::query(ctx, conn).map_err(Into::into),
        |rows, params| layers::water_lines::render(&ctx, context, rows, params.svg_repo),
    );

    // waterareas
    prefetcher.add(
        "water_areas",
        |ctx, conn| layers::water_areas::query(ctx, conn).map_err(Into::into),
        |rows, _params| layers::water_areas::render(&ctx, context, rows),
    );

    if zoom >= 15 {
        // osm_landcovers (bridge_areas)
        prefetcher.add(
            "bridge_areas",
            |ctx, conn| layers::bridge_areas::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::bridge_areas::render(&ctx, context, rows, false),
        );
    }

    if zoom >= 16 {
        // pois
        prefetcher.add(
            "trees",
            |ctx, conn| layers::trees::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::trees::render(&ctx, context, rows, params.svg_repo),
        );
    }

    if zoom >= 12
        && let Some(handle) = feature_lines.clone()
    {
        prefetcher.add_with(handle, |rows, params| {
            layers::feature_lines::render(
                &ctx,
                context,
                2,
                &rows,
                params.svg_repo,
                do_shading.then_some(params.hsd).flatten(),
            )
        });
    }

    if zoom >= 16 {
        // roads
        prefetcher.add(
            "embankments",
            |ctx, conn| layers::embankments::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::embankments::render(&ctx, context, rows, params.svg_repo),
        );
    }

    if zoom >= 8 {
        // roads
        prefetcher.add(
            "roads",
            |ctx, conn| layers::roads::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::roads::render(&ctx, context, rows, params.svg_repo),
        );
    }

    if zoom >= 14 {
        // roads
        prefetcher.add(
            "road_access_restrictions",
            |ctx, conn| layers::road_access_restrictions::query(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::road_access_restrictions::render(&ctx, context, rows, params.svg_repo)
            },
        );
    }

    if zoom >= 11
        && let Some(handle) = feature_lines.clone()
    {
        prefetcher.add_with(handle, |rows, params| {
            layers::feature_lines::render(&ctx, context, 3, &rows, params.svg_repo, None)
        });
    }

    const CONTOUR_COUNTRIES: [Option<&'static str>; 10] = [
        Some("at"),
        Some("it"),
        Some("ch"),
        Some("si"),
        Some("cz"),
        Some("pl"),
        Some("sk"),
        Some("fr"),
        Some("no"),
        None,
    ];

    if do_shading || do_contours {
        let bridge = prefetcher.prefetch("bridge_areas_mask", |ctx, conn| {
            layers::bridge_areas::query(ctx, conn).map_err(Into::into)
        });

        let contours = CONTOUR_COUNTRIES.map(|country| {
            (
                country,
                prefetcher.prefetch("contours", move |ctx, conn| {
                    layers::contours::query(ctx, conn, country.map(|s| s as &str))
                        .map_err(Into::into)
                }),
            )
        });

        let ctx_arc = ctx.clone();

        prefetcher.push(move |params| {
            let Some(hsd) = params.hsd else { return Ok(()) };

            let bridge_rows = bridge.consume()?;

            let mut contour_rows: HashMap<Option<&'static str>, Vec<Feature>> = HashMap::new();

            for (country, handle) in contours {
                contour_rows.insert(country, handle.consume()?);
            }
            layers::shading_and_contours::render(
                &ctx_arc,
                context,
                bridge_rows,
                contour_rows,
                hsd,
                do_shading,
                do_contours,
            )
            .with_layer("shading_and_contours")
        });
    }

    if zoom >= 12 {
        // osm_power_generators (solar_power_plants)
        prefetcher.add(
            "solar_power_plants",
            |ctx, conn| layers::solar_power_plants::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::solar_power_plants::render(&ctx, context, rows),
        );
    }

    if zoom >= 13 {
        // osm_buildings (buildings)
        prefetcher.add(
            "buildings",
            |ctx, conn| layers::buildings::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::buildings::render(&ctx, context, rows),
        );
    }

    if zoom >= 12
        && let Some(handle) = feature_lines.clone()
    {
        prefetcher.add_with(handle, |rows, params| {
            layers::feature_lines::render(&ctx, context, 4, &rows, params.svg_repo, None)
        });
    }

    if zoom >= 14 {
        prefetcher.add(
            "power_towers_poles",
            |ctx, conn| layers::power_towers_poles::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::power_towers_poles::render(&ctx, context, rows),
        );
    }

    if zoom >= 8 {
        prefetcher.add(
            "protected_areas_areas",
            |ctx, conn| layers::protected_areas::query_areas(ctx, conn).map_err(Into::into),
            |rows, _params| layers::protected_areas::render_areas(&ctx, context, rows),
        );
        prefetcher.add(
            "protected_areas_borders",
            |ctx, conn| layers::protected_areas::query_borders(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::protected_areas::render_borders(&ctx, context, rows, params.svg_repo)
            },
        );
    }

    if zoom >= 13 {
        prefetcher.add(
            "special_parks",
            |ctx, conn| layers::special_parks::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::special_parks::render(&ctx, context, rows),
        );
    }

    if zoom >= 10 {
        prefetcher.add(
            "military_areas",
            |ctx, conn| layers::military_areas::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::military_areas::render(&ctx, context, rows),
        );
    }

    if zoom >= 8 && to_render.contains(&RenderLayer::CountryBorders) {
        prefetcher.add(
            "borders",
            |ctx, conn| layers::borders::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::borders::render(&ctx, context, rows),
        );
    }

    {
        let to_render = to_render.clone();
        let to_render1 = to_render.clone();

        let min_zoom = if to_render.contains(&RenderLayer::RoutesHikingKst) {
            8
        } else {
            9
        };

        if zoom >= min_zoom {
            prefetcher.add(
                "routes_marking",
                move |ctx, conn| {
                    layers::routes::query_marking(ctx, conn, &to_render).map_err(Into::into)
                },
                |rows, params| {
                    layers::routes::render_marking(&ctx, context, rows, to_render1, params.svg_repo)
                },
            );
        }
    }

    if (9..=11).contains(&zoom) && to_render.contains(&RenderLayer::Geonames) {
        prefetcher.add(
            "geonames",
            |ctx, conn| layers::geonames::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::geonames::render(&ctx, context, rows),
        );
    }

    if zoom >= 14 {
        prefetcher.add(
            "fixmes_points",
            |ctx, conn| layers::fixmes::query_points(ctx, conn).map_err(Into::into),
            |rows, params| layers::fixmes::render_points(&ctx, context, rows, params.svg_repo),
        );
        prefetcher.add(
            "fixmes_lines",
            |ctx, conn| layers::fixmes::query_lines(ctx, conn).map_err(Into::into),
            |rows, params| layers::fixmes::render_lines(&ctx, context, rows, params.svg_repo),
        );
    }

    if zoom >= 13 {
        let opacity = 0.5 - (zoom as f64 - 13.0) / 10.0;

        prefetcher.push(move |_params| {
            context.push_group();
            Ok(())
        });

        prefetcher.add(
            "valleys",
            |ctx, conn| layers::valleys_ridges::query_valleys(ctx, conn).map_err(Into::into),
            |rows, _params| layers::valleys_ridges::render_valleys(&ctx, context, rows),
        );

        prefetcher.add(
            "ridges",
            |ctx, conn| layers::valleys_ridges::query_ridges(ctx, conn).map_err(Into::into),
            |rows, _params| layers::valleys_ridges::render_ridges(&ctx, context, rows),
        );

        prefetcher.push(move |_params| {
            context.pop_group_to_source()?;
            context.paint_with_alpha(opacity)?;
            Ok(())
        });
    }

    if (8..=14).contains(&zoom) {
        prefetcher.add(
            "place_names",
            |ctx, conn| layers::place_names::query(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::place_names::render(&ctx, context, rows, &mut Some(params.collision))
            },
        );
    }

    if (8..=10).contains(&zoom) {
        prefetcher.add(
            "national_park_names",
            |ctx, conn| layers::national_park_names::query(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::national_park_names::render(&ctx, context, rows, params.collision)
            },
        );
    }

    if (13..=16).contains(&zoom) {
        prefetcher.add(
            "special_park_names",
            |ctx, conn| layers::special_park_names::query(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::special_park_names::render(&ctx, context, rows, params.collision)
            },
        );
    }

    if zoom >= 10 {
        let kst = to_render.contains(&RenderLayer::RoutesHikingKst);
        prefetcher.add(
            "pois",
            move |ctx, conn| layers::pois::query(ctx, conn, kst).map_err(Into::into),
            |rows, params| {
                layers::pois::render(&ctx, context, rows, params.collision, params.svg_repo)
            },
        );
    }

    if zoom >= 10 {
        prefetcher.add(
            "water_area_names",
            |ctx, conn| layers::water_area_names::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::water_area_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 17 {
        prefetcher.add(
            "building_names",
            |ctx, conn| layers::building_names::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::building_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 12 {
        prefetcher.add(
            "bordered_area_names_centroids",
            |ctx, conn| layers::bordered_area_names::query_centroids(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::bordered_area_names::render_centroids(&ctx, context, rows, params.collision)
            },
        );
        prefetcher.add(
            "bordered_area_names_borders",
            |ctx, conn| layers::bordered_area_names::query_borders(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::bordered_area_names::render_borders(&ctx, context, rows, params.collision)
            },
        );
        prefetcher.add(
            "landcover_names",
            |ctx, conn| layers::landcover_names::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::landcover_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 15 {
        prefetcher.add(
            "locality_names",
            |ctx, conn| layers::locality_names::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::locality_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 18 {
        prefetcher.add(
            "housenumbers",
            |ctx, conn| layers::housenumbers::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::housenumbers::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 15 {
        prefetcher.add(
            "highway_names",
            |ctx, conn| layers::highway_names::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::highway_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 14 {
        let render_clone = to_render.clone();
        prefetcher.add(
            "routes_labels",
            move |ctx, conn| {
                layers::routes::query_labels(ctx, conn, &render_clone).map_err(Into::into)
            },
            |rows, params| layers::routes::render_labels(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 16 {
        prefetcher.add(
            "aerialway_names",
            |ctx, conn| layers::aerialway_names::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::aerialway_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 12 {
        prefetcher.add(
            "water_line_names",
            |ctx, conn| layers::water_line_names::query(ctx, conn).map_err(Into::into),
            |rows, params| layers::water_line_names::render(&ctx, context, rows, params.collision),
        );
    }

    if (15..=17).contains(&zoom) {
        prefetcher.add(
            "place_names_highzoom",
            |ctx, conn| layers::place_names::query(ctx, conn).map_err(Into::into),
            |rows, params| {
                layers::place_names::render(&ctx, context, rows, &mut Some(params.collision))
            },
        );
    }

    if zoom < 8 && to_render.contains(&RenderLayer::CountryNames) {
        let rect = ctx.bbox.project_to_tile(&ctx.tile_projector);

        prefetcher.push(move |_params| {
            context.save()?;
            context.rectangle(rect.min().x, rect.min().y, rect.width(), rect.height());
            context.set_source_color_a(colors::WHITE, 0.33);
            context.fill()?;
            context.restore()?;

            Ok(())
        });

        prefetcher.add(
            "borders",
            |ctx, conn| layers::borders::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::borders::render(&ctx, context, rows),
        );

        prefetcher.add(
            "country_names",
            |ctx, conn| layers::country_names::query(ctx, conn).map_err(Into::into),
            |rows, _params| layers::country_names::render(&ctx, context, rows),
        );
    }

    if let Some(ref coverage_geometry) = coverage_geometry {
        prefetcher.push(|_params| {
            layers::blur_edges::render(&ctx, context, coverage_geometry)
                .with_layer("blur_edges")?;
            context.pop_group_to_source()?;
            context.paint()?;

            Ok(())
        });
    }

    if let Some(ref features) = request.featues {
        prefetcher.push(|_params| {
            layers::custom::render(&ctx, context, features).with_layer("custom")?;

            Ok(())
        });
    }

    prefetcher.run(svg_repo, hillshading_datasets.as_deref_mut(), collision)?;

    if let Some(hillshading_datasets) = hillshading_datasets {
        hillshading_datasets.evict_unused();
    }

    Ok(())
}
