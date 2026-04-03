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
use deadpool_postgres::Pool;
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use geo::Rect;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio_postgres::Row;

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

    #[error("Render task panicked")]
    TaskPanic,

    #[error("DB pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
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

struct Params<'p, 'ctx> {
    collision: &'p mut Collision<'ctx>,
    svg_repo: &'p mut SvgRepo,
    hsd: Option<&'p mut HillshadingDatasets>,
}

/// Key used to index batch results (contours + bridge).
type BatchKey = Option<&'static str>;

enum PendingLayer<'a> {
    /// A DB query running as its own tokio task (own pool connection → true parallelism).
    Query {
        name: &'static str,
        jh: JoinHandle<Result<Vec<Feature>, LayerRenderError>>,
        render_fn: Box<dyn FnOnce(Vec<Feature>, Params) -> LayerRenderResult + 'a>,
    },
    /// Multiple independent pre-spawned queries collected into one render call
    /// (used for contours + bridge_areas_mask running in parallel).
    Batch {
        handles: Vec<(BatchKey, JoinHandle<Result<Vec<Feature>, LayerRenderError>>)>,
        render_fn: Box<dyn FnOnce(HashMap<BatchKey, Vec<Feature>>, Params) -> LayerRenderResult + 'a>,
    },
    /// Render-only step (push_group, pop_group, blur_edges, custom, …)
    Push(Box<dyn FnOnce(Params) -> Result<(), RenderError> + 'a>),
    /// Legend path: features pre-built, render directly.
    Legend {
        name: &'static str,
        features: Vec<Feature>,
        render_fn: Box<dyn FnOnce(Vec<Feature>, Params) -> LayerRenderResult + 'a>,
    },
}

struct Prefetcher<'a> {
    pool: Pool,
    handle: Handle,
    ctx: Arc<Ctx>,
    layers: Vec<PendingLayer<'a>>,
}

impl<'a> Prefetcher<'a> {
    fn new(pool: Pool, handle: Handle, ctx: Arc<Ctx>) -> Self {
        Self {
            pool,
            handle,
            ctx,
            layers: Vec::new(),
        }
    }

    /// Add a layer with a DB query.
    /// Each query spawns its own tokio task with its own pool connection,
    /// so all queries run in parallel on the DB.
    fn add(
        &mut self,
        name: &'static str,
        legend_name: Option<&'static str>,
        query_fn: impl FnOnce(Arc<Ctx>, deadpool_postgres::Object) -> BoxFuture<'static, Result<Vec<Row>, tokio_postgres::Error>> + Send + 'static,
        render_fn: impl FnOnce(Vec<Feature>, Params) -> LayerRenderResult + 'a,
    ) {
        if let Some(ref legend) = self.ctx.legend {
            let key = legend_name.unwrap_or(name);
            if let Some(legend) = legend.get(key) {
                let features = legend
                    .iter()
                    .map(|props| Feature::LegendData(props.clone()))
                    .collect();
                self.layers.push(PendingLayer::Legend {
                    name,
                    features,
                    render_fn: Box::new(render_fn),
                });
            }
            return;
        }

        let pool = self.pool.clone();
        let ctx = self.ctx.clone();

        let jh = self.handle.spawn(async move {
            let conn = pool.get().await.map_err(LayerRenderError::from)?;
            let rows = query_fn(ctx, conn).await.map_err(LayerRenderError::from)?;
            Ok::<Vec<Feature>, LayerRenderError>(rows.into_iter().map(Feature::from).collect())
        });

        self.layers.push(PendingLayer::Query {
            name,
            jh,
            render_fn: Box::new(render_fn),
        });
    }

    /// Spawn an independent background task for a batch (contours/bridge).
    fn prefetch(
        &self,
        query_fn: impl FnOnce(Arc<Ctx>, deadpool_postgres::Object) -> BoxFuture<'static, Result<Vec<Feature>, LayerRenderError>> + Send + 'static,
    ) -> JoinHandle<Result<Vec<Feature>, LayerRenderError>> {
        let pool = self.pool.clone();
        let ctx = self.ctx.clone();
        self.handle.spawn(async move {
            let conn = pool.get().await.map_err(LayerRenderError::from)?;
            query_fn(ctx, conn).await
        })
    }

    fn push(&mut self, render_fn: impl FnOnce(Params) -> Result<(), RenderError> + 'a) {
        self.layers.push(PendingLayer::Push(Box::new(render_fn)));
    }

    fn run(
        self,
        svg_repo: &mut SvgRepo,
        mut hsd: Option<&mut HillshadingDatasets>,
        collision: &mut Collision,
    ) -> Result<(), RenderError> {
        self.handle.block_on(async move {
            for layer in self.layers {
                let params = Params {
                    svg_repo,
                    hsd: hsd.as_deref_mut(),
                    collision,
                };

                match layer {
                    PendingLayer::Query { name, jh, render_fn } => {
                        // Awaiting in order: while we wait for this result, all other tasks
                        // are running in parallel on the tokio executor.
                        let features = jh
                            .await
                            .map_err(|_| RenderError::TaskPanic)?
                            .with_layer(name)?;
                        render_fn(features, params).with_layer(name)?;
                    }
                    PendingLayer::Batch { handles, render_fn } => {
                        let mut results = HashMap::with_capacity(handles.len());
                        for (key, jh) in handles {
                            let features = jh
                                .await
                                .map_err(|_| RenderError::TaskPanic)?
                                .with_layer("batch")?;
                            results.insert(key, features);
                        }
                        render_fn(results, params).with_layer("batch")?;
                    }
                    PendingLayer::Legend { name, features, render_fn } => {
                        render_fn(features, params).with_layer(name)?;
                    }
                    PendingLayer::Push(f) => {
                        f(params)?;
                    }
                }
            }

            Ok(())
        })
    }
}

pub fn render(
    surface: &Surface,
    request: &RenderRequest,
    pool: Pool,
    handle: Handle,
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

    let mut prefetcher = Prefetcher::new(pool.clone(), handle.clone(), ctx.clone());

    if request.legend.is_none() {
        prefetcher.add(
            "sea",
            None,
            |ctx, conn| async move { layers::sea::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::sea::render(&ctx, context, rows),
        );
    }

    prefetcher.add(
        "landcovers",
        None,
        |ctx, conn| async move { layers::landcover::query(&ctx, &conn).await }.boxed(),
        |rows, params| layers::landcover::render(&ctx, context, rows, params.svg_repo),
    );

    // feature_lines is queried per render stage (up to 4×). All tasks run in parallel
    // so the cost is a pool connection rather than a round-trip per query.
    if zoom >= 13 {
        prefetcher.add(
            "feature_lines_1",
            Some("feature_lines"),
            |ctx, conn| async move { layers::feature_lines::query(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::feature_lines::render(&ctx, context, 1, &rows, params.svg_repo, None)
            },
        );
    }

    prefetcher.add(
        "water_lines",
        None,
        |ctx, conn| async move { layers::water_lines::query(&ctx, &conn).await }.boxed(),
        |rows, params| layers::water_lines::render(&ctx, context, rows, params.svg_repo),
    );

    prefetcher.add(
        "water_areas",
        None,
        |ctx, conn| async move { layers::water_areas::query(&ctx, &conn).await }.boxed(),
        |rows, _params| layers::water_areas::render(&ctx, context, rows),
    );

    if zoom >= 15 {
        prefetcher.add(
            "bridge_areas",
            None,
            |ctx, conn| async move { layers::bridge_areas::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::bridge_areas::render(&ctx, context, rows, false),
        );
    }

    if zoom >= 16 {
        prefetcher.add(
            "trees",
            None,
            |ctx, conn| async move { layers::trees::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::trees::render(&ctx, context, rows, params.svg_repo),
        );
    }

    if zoom >= 12 {
        prefetcher.add(
            "feature_lines_2",
            Some("feature_lines"),
            |ctx, conn| async move { layers::feature_lines::query(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::feature_lines::render(
                    &ctx,
                    context,
                    2,
                    &rows,
                    params.svg_repo,
                    do_shading.then_some(params.hsd).flatten(),
                )
            },
        );
    }

    if zoom >= 16 {
        prefetcher.add(
            "embankments",
            None,
            |ctx, conn| async move { layers::embankments::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::embankments::render(&ctx, context, rows, params.svg_repo),
        );
    }

    if zoom >= 8 {
        prefetcher.add(
            "roads",
            None,
            |ctx, conn| async move { layers::roads::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::roads::render(&ctx, context, rows, params.svg_repo),
        );
    }

    if zoom >= 14 {
        prefetcher.add(
            "road_access_restrictions",
            None,
            |ctx, conn| async move {
                layers::road_access_restrictions::query(&ctx, &conn).await
            }
            .boxed(),
            |rows, params| {
                layers::road_access_restrictions::render(&ctx, context, rows, params.svg_repo)
            },
        );
    }

    if zoom >= 11 {
        prefetcher.add(
            "feature_lines_3",
            Some("feature_lines"),
            |ctx, conn| async move { layers::feature_lines::query(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::feature_lines::render(&ctx, context, 3, &rows, params.svg_repo, None)
            },
        );
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
        let bridge_handle = prefetcher.prefetch(|ctx, conn| {
            async move {
                Ok(layers::bridge_areas::query(&ctx, &conn)
                    .await?
                    .into_iter()
                    .map(Feature::from)
                    .collect())
            }
            .boxed()
        });

        let contour_handles: [(BatchKey, JoinHandle<_>); 10] = CONTOUR_COUNTRIES.map(|country| {
            (
                country,
                prefetcher.prefetch(move |ctx, conn| {
                    async move {
                        Ok(layers::contours::query(&ctx, &conn, country)
                            .await?
                            .into_iter()
                            .map(Feature::from)
                            .collect())
                    }
                    .boxed()
                }),
            )
        });

        let ctx_arc = ctx.clone();

        let mut handles: Vec<(BatchKey, JoinHandle<_>)> = Vec::with_capacity(11);
        handles.push((Some("__bridge__"), bridge_handle));
        for (key, jh) in contour_handles {
            handles.push((key, jh));
        }

        prefetcher.layers.push(PendingLayer::Batch {
            handles,
            render_fn: Box::new(move |mut results, params| {
                let Some(hsd) = params.hsd else { return Ok(()) };

                let bridge_rows = results.remove(&Some("__bridge__")).unwrap_or_default();

                let mut contour_rows: HashMap<Option<&'static str>, Vec<Feature>> =
                    HashMap::new();

                for country in CONTOUR_COUNTRIES {
                    contour_rows.insert(country, results.remove(&country).unwrap_or_default());
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
            }),
        });
    }

    if zoom >= 12 {
        prefetcher.add(
            "solar_power_plants",
            None,
            |ctx, conn| async move { layers::solar_power_plants::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::solar_power_plants::render(&ctx, context, rows),
        );
    }

    if zoom >= 13 {
        prefetcher.add(
            "buildings",
            None,
            |ctx, conn| async move { layers::buildings::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::buildings::render(&ctx, context, rows),
        );
    }

    if zoom >= 12 {
        prefetcher.add(
            "feature_lines_4",
            Some("feature_lines"),
            |ctx, conn| async move { layers::feature_lines::query(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::feature_lines::render(&ctx, context, 4, &rows, params.svg_repo, None)
            },
        );
    }

    if zoom >= 14 {
        prefetcher.add(
            "power_towers_poles",
            None,
            |ctx, conn| async move { layers::power_towers_poles::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::power_towers_poles::render(&ctx, context, rows),
        );
    }

    if zoom >= 8 {
        prefetcher.add(
            "protected_areas_areas",
            Some("protected_areas"),
            |ctx, conn| async move { layers::protected_areas::query_areas(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::protected_areas::render_areas(&ctx, context, rows),
        );
        prefetcher.add(
            "protected_areas_borders",
            Some("protected_areas"),
            |ctx, conn| async move { layers::protected_areas::query_borders(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::protected_areas::render_borders(&ctx, context, rows, params.svg_repo)
            },
        );
    }

    if zoom >= 13 {
        prefetcher.add(
            "special_parks",
            None,
            |ctx, conn| async move { layers::special_parks::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::special_parks::render(&ctx, context, rows),
        );
    }

    if zoom >= 10 {
        prefetcher.add(
            "military_areas",
            None,
            |ctx, conn| async move { layers::military_areas::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::military_areas::render(&ctx, context, rows),
        );
    }

    if zoom >= 8 && to_render.contains(&RenderLayer::CountryBorders) {
        prefetcher.add(
            "borders",
            None,
            |ctx, conn| async move { layers::borders::query(&ctx, &conn).await }.boxed(),
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
                Some("routes"),
                move |ctx, conn| {
                    async move {
                        layers::routes::query_marking(&ctx, &conn, &to_render).await
                    }
                    .boxed()
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
            None,
            |ctx, conn| async move { layers::geonames::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::geonames::render(&ctx, context, rows),
        );
    }

    if zoom >= 14 {
        prefetcher.add(
            "fixmes_points",
            Some("fixmes"),
            |ctx, conn| async move { layers::fixmes::query_points(&ctx, &conn).await }.boxed(),
            |rows, params| layers::fixmes::render_points(&ctx, context, rows, params.svg_repo),
        );
        prefetcher.add(
            "fixmes_line",
            None,
            |ctx, conn| async move { layers::fixmes::query_lines(&ctx, &conn).await }.boxed(),
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
            Some("valleys_ridges"),
            |ctx, conn| async move { layers::valleys_ridges::query_valleys(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::valleys_ridges::render_valleys(&ctx, context, rows),
        );

        prefetcher.add(
            "ridges",
            Some("valleys_ridges"),
            |ctx, conn| async move { layers::valleys_ridges::query_ridges(&ctx, &conn).await }.boxed(),
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
            None,
            |ctx, conn| async move { layers::place_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::place_names::render(&ctx, context, rows, &mut Some(params.collision))
            },
        );
    }

    if (8..=10).contains(&zoom) {
        prefetcher.add(
            "national_park_names",
            None,
            |ctx, conn| async move { layers::national_park_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::national_park_names::render(&ctx, context, rows, params.collision)
            },
        );
    }

    if (13..=16).contains(&zoom) {
        prefetcher.add(
            "special_park_names",
            None,
            |ctx, conn| async move { layers::special_park_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| {
                layers::special_park_names::render(&ctx, context, rows, params.collision)
            },
        );
    }

    if zoom >= 10 {
        let kst = to_render.contains(&RenderLayer::RoutesHikingKst);
        prefetcher.add(
            "pois",
            None,
            move |ctx, conn| async move { layers::pois::query(&ctx, &conn, kst).await }.boxed(),
            |rows, params| {
                layers::pois::render(&ctx, context, rows, params.collision, params.svg_repo)
            },
        );
    }

    if zoom >= 10 {
        prefetcher.add(
            "water_area_names",
            Some("water_areas"),
            |ctx, conn| async move { layers::water_area_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::water_area_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 17 {
        prefetcher.add(
            "building_names",
            None,
            |ctx, conn| async move { layers::building_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::building_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 12 {
        prefetcher.add(
            "bordered_area_names_centroids",
            Some("protected_areas"),
            |ctx, conn| async move {
                layers::bordered_area_names::query_centroids(&ctx, &conn).await
            }
            .boxed(),
            |rows, params| {
                layers::bordered_area_names::render_centroids(&ctx, context, rows, params.collision)
            },
        );
        prefetcher.add(
            "bordered_area_names_borders",
            Some("protected_areas"),
            |ctx, conn| async move {
                layers::bordered_area_names::query_borders(&ctx, &conn).await
            }
            .boxed(),
            |rows, params| {
                layers::bordered_area_names::render_borders(&ctx, context, rows, params.collision)
            },
        );
        prefetcher.add(
            "landcover_names",
            Some("landcovers"),
            |ctx, conn| async move { layers::landcover_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::landcover_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 15 {
        prefetcher.add(
            "locality_names",
            None,
            |ctx, conn| async move { layers::locality_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::locality_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 18 {
        prefetcher.add(
            "housenumbers",
            None,
            |ctx, conn| async move { layers::housenumbers::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::housenumbers::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 15 {
        prefetcher.add(
            "highway_names",
            Some("roads"),
            |ctx, conn| async move { layers::highway_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::highway_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 14 {
        let render_clone = to_render.clone();
        prefetcher.add(
            "routes_labels",
            Some("routes"),
            move |ctx, conn| {
                async move {
                    layers::routes::query_labels(&ctx, &conn, &render_clone).await
                }
                .boxed()
            },
            |rows, params| layers::routes::render_labels(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 16 {
        prefetcher.add(
            "aerialway_names",
            Some("feature_lines"),
            |ctx, conn| async move { layers::aerialway_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::aerialway_names::render(&ctx, context, rows, params.collision),
        );
    }

    if zoom >= 12 {
        prefetcher.add(
            "water_line_names",
            Some("water_lines"),
            |ctx, conn| async move { layers::water_line_names::query(&ctx, &conn).await }.boxed(),
            |rows, params| layers::water_line_names::render(&ctx, context, rows, params.collision),
        );
    }

    if (15..=17).contains(&zoom) {
        prefetcher.add(
            "place_names_highzoom",
            Some("place_names"),
            |ctx, conn| async move { layers::place_names::query(&ctx, &conn).await }.boxed(),
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
            "country_borders",
            None,
            |ctx, conn| async move { layers::borders::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::borders::render(&ctx, context, rows),
        );

        prefetcher.add(
            "country_names",
            None,
            |ctx, conn| async move { layers::country_names::query(&ctx, &conn).await }.boxed(),
            |rows, _params| layers::country_names::render(&ctx, context, rows),
        );
    }

    if let Some(coverage_geometry) = coverage_geometry {
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
