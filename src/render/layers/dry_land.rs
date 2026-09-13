//! Keeps hillshading and contours off the sea and permanent water, where DEMs still
//! report elevations.

use crate::render::{
    ctx::Ctx, draw::path_geom::path_geometry, layer_render_error::LayerRenderResult,
    layers::contours,
};
use cairo::{Context, Format, ImageSurface, SurfacePattern};
use geo::{BoundingRect, Contains, Coord, Geometry, Intersects, Point, Rect};

/// Below it there are no contours, and offshore shading bleed is sub-pixel.
pub const MIN_ZOOM: u8 = contours::MIN_ZOOM;

/// How far past the tile land and water are fetched while contour labels are drawn. A drawn
/// label reaches ~45 px out, and neighbouring tiles must judge it alike.
pub const LABEL_MARGIN_PX: f64 = 64.0;

type Piece = (Rect<f64>, Geometry);

pub struct DryLand {
    land: Vec<Piece>,
    water: Vec<Piece>,
}

fn pieces(geometries: Vec<Geometry>) -> Vec<Piece> {
    geometries
        .into_iter()
        .filter_map(|geom| geom.bounding_rect().map(|bbox| (bbox, geom)))
        .collect()
}

fn on_land(land: &[Piece], point: Coord<f64>) -> bool {
    let as_point = Point::from(point);

    land.iter()
        .any(|(bbox, geom)| bbox.contains(&point) && geom.contains(&as_point))
}

impl DryLand {
    /// Takes the land the sea fill projected.
    pub fn new(land: Vec<Geometry>) -> Self {
        Self {
            land: pieces(land),
            water: Vec::new(),
        }
    }

    /// Takes the permanent water the water fill projected.
    pub fn set_water(&mut self, water: Vec<Geometry>) {
        self.water = pieces(water);
    }

    pub const fn has_land(&self) -> bool {
        !self.land.is_empty()
    }

    /// Whether a label with this bounding box comes out whole.
    pub fn allows_label(&self, bbox: &Rect<f64>) -> bool {
        if self
            .water
            .iter()
            .any(|(piece_bbox, geom)| piece_bbox.intersects(bbox) && geom.intersects(bbox))
        {
            return false;
        }

        // Sampled, not tested as a box: a box can lie on land while no single subdivided
        // piece contains it.
        let (min, max) = (bbox.min(), bbox.max());
        let mid = bbox.center();

        [
            min,
            Coord { x: mid.x, y: min.y },
            Coord { x: max.x, y: min.y },
            Coord { x: min.x, y: mid.y },
            Coord { x: max.x, y: mid.y },
            Coord { x: min.x, y: max.y },
            Coord { x: mid.x, y: max.y },
            max,
        ]
        .into_iter()
        .all(|point| on_land(&self.land, point))
    }

    /// Draws into a group, then masks it to land minus water in a single composite. Not a
    /// clip: cairo re-tessellates a clip this detailed on every paint under it (#96).
    pub fn masked(
        &self,
        ctx: &Ctx,
        context: &Context,
        draw: impl FnOnce() -> LayerRenderResult,
    ) -> LayerRenderResult {
        context.push_group(); // dry-land

        draw()?;

        let _span = tracy_client::span!("dry_land::mask");

        context.pop_group_to_source()?; // dry-land

        let mask = ImageSurface::create(
            Format::A8,
            (f64::from(ctx.size.width) * ctx.scale) as i32,
            (f64::from(ctx.size.height) * ctx.scale) as i32,
        )?;

        {
            let mask_context = Context::new(&mask)?;

            mask_context.scale(ctx.scale, ctx.scale);

            // One piece at a time, as the sea fill does; the buffered overlap hides the seams.
            for (_, geom) in &self.land {
                path_geometry(&mask_context, geom);

                mask_context.fill()?;
            }

            mask_context.set_operator(cairo::Operator::Clear);

            for (_, geom) in &self.water {
                path_geometry(&mask_context, geom);

                mask_context.fill()?; // one at a time, so overlapping polygons cannot cancel out
            }
        }

        let pattern = SurfacePattern::create(&mask);

        #[allow(clippy::float_cmp)] // exact identity check: skip transform when scale is 1.0
        if ctx.scale != 1.0 {
            pattern.set_matrix(cairo::Matrix::new(ctx.scale, 0.0, 0.0, ctx.scale, 0.0, 0.0));
        }

        context.mask(&pattern)?;

        Ok(())
    }
}
