//! Keeps hillshading and contours on dry land.
//!
//! A DEM answers offshore as readily as it does on land — GEDTM30, which covers every
//! country without a national model, returns a flat zero over water — so shading spills
//! into the sea and a 0 m contour is drawn across it. Water mapped as `natural=water`
//! instead of as coastline (lagoons, harbour basins, lakes) has the same problem.
//!
//! Both are cut away against the very geometry the sea and water layers fill, so a cut
//! can never disagree with the fill drawn over it. Land is a clip; water has to be a
//! mask, because a clip cannot subtract.

use crate::render::{
    Feature,
    ctx::Ctx,
    draw::path_geom::path_geometry,
    layer_render_error::{LayerRenderError, LayerRenderResult},
    layers::{sea, water_areas},
};
use cairo::{Context, Format, ImageSurface, SurfacePattern};
use geo::{BoundingRect, Contains, Coord, Geometry, Intersects, Point, Rect};

/// How far beyond the tile the land and water geometry is fetched, in tile pixels.
///
/// Both tiles drawing a label that spans their shared edge must reach the same verdict on
/// it, or they render different halves of one label — so the geometry has to reach as far
/// out as a drawn label can. A label is still drawn while its box merely touches the clip
/// grown by `halo_width + font size` (13.5 px for contours), and the box itself runs some
/// 30 px, which puts the far end around 45 px out. 64 leaves room above that.
pub const KNOWN_MARGIN_PX: f64 = 64.0;

/// A projected polygon and its bounding box, kept for the point tests.
type Piece = (Rect<f64>, Geometry);

/// The land left to draw on, and the state needed to undo the clip in [`end`].
pub struct DryLand {
    land: Vec<Piece>,
    water: Vec<Piece>,
    /// Where `land` and `water` describe the world; outside it nothing is known.
    known: Rect<f64>,
    /// Whether [`begin`] opened a group for the water mask.
    masked: bool,
}

impl DryLand {
    /// Whether any land is left. `false` means open sea: the caller can skip its work
    /// entirely, since everything it draws would be clipped away.
    pub const fn has_land(&self) -> bool {
        !self.land.is_empty()
    }

    /// Whether a label with this bounding box would come out whole. Placing a label that
    /// straddles the coastline draws sliced glyphs, the clip cutting through them.
    pub fn allows_label(&self, bbox: &Rect<f64>) -> bool {
        // Water can be tested as a whole box: any overlap at all, however thin, cuts a
        // glyph. Nothing forces water polygons into a partition, so one piece is enough.
        if self
            .water
            .iter()
            .any(|(piece_bbox, geom)| piece_bbox.intersects(bbox) && geom.intersects(bbox))
        {
            return false;
        }

        // Land has to be sampled point by point: it arrives as `ST_Subdivide` pieces, so a
        // box can lie wholly on land while no single piece contains it. Corners alone would
        // miss an inlet narrower than a glyph passing between them, hence the edge midpoints.
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
        // Beyond the queried area, leave the verdict to the tile that knows.
        .all(|point| !self.known.contains(&point) || on_land(&self.land, point))
    }
}

fn on_land(pieces: &[Piece], point: Coord<f64>) -> bool {
    let as_point = Point::from(point);

    pieces
        .iter()
        .any(|(bbox, geom)| bbox.contains(&point) && geom.contains(&as_point))
}

fn collect(pieces: &mut Vec<Piece>, geom: &Geometry) {
    if let Some(bbox) = geom.bounding_rect() {
        pieces.push((bbox, geom.clone()));
    }
}

/// Clips to land, and opens a group for the water mask when there is water to cut.
/// Every [`DryLand`] this hands back must be passed to [`end`]; an error leaves the
/// context as it found it.
pub fn begin(
    ctx: &Ctx,
    context: &Context,
    land_rows: &[Feature],
    water_rows: &[Feature],
) -> Result<DryLand, LayerRenderError> {
    let _span = tracy_client::span!("dry_land::begin");

    let mut land = Vec::new();

    sea::for_each_land(ctx, land_rows, |geom| {
        collect(&mut land, geom);

        Ok(())
    })?;

    // Water is only cut from z12 up — where contours start, and where the water layer
    // draws the ungeneralized geometry this mask has to agree with.
    let mut water = Vec::new();

    if ctx.zoom >= 12 && !land.is_empty() {
        water_areas::for_each_permanent(ctx, water_rows, |geom| {
            collect(&mut water, geom);

            Ok(())
        })?;
    }

    // Everything that can fail is behind us: from here the cairo state is left changed,
    // and only `end` puts it back.
    context.save()?;

    for (_, geom) in &land {
        path_geometry(context, geom);
    }

    context.clip();

    let masked = !water.is_empty();

    if masked {
        context.push_group(); // dry-land
    }

    let margin = KNOWN_MARGIN_PX;

    Ok(DryLand {
        land,
        water,
        known: Rect::new(
            Coord {
                x: -margin,
                y: -margin,
            },
            Coord {
                x: f64::from(ctx.size.width) + margin,
                y: f64::from(ctx.size.height) + margin,
            },
        ),
        masked,
    })
}

/// Punches the water out of what was drawn since [`begin`], and drops the land clip.
pub fn end(ctx: &Ctx, context: &Context, dry_land: &DryLand) -> LayerRenderResult {
    let _span = tracy_client::span!("dry_land::end");

    if dry_land.masked {
        context.pop_group_to_source()?; // dry-land

        let mask = ImageSurface::create(
            Format::A8,
            (f64::from(ctx.size.width) * ctx.scale) as i32,
            (f64::from(ctx.size.height) * ctx.scale) as i32,
        )?;

        {
            let mask_context = Context::new(&mask)?;

            mask_context.paint()?; // opaque: everything drawn survives

            mask_context.scale(ctx.scale, ctx.scale);
            mask_context.set_operator(cairo::Operator::Clear);

            for (_, geom) in &dry_land.water {
                path_geometry(&mask_context, geom);

                mask_context.fill()?; // one at a time: overlapping polygons must not cancel
            }
        }

        let pattern = SurfacePattern::create(&mask);

        #[allow(clippy::float_cmp)] // exact identity check: skip transform when scale is 1.0
        if ctx.scale != 1.0 {
            pattern.set_matrix(cairo::Matrix::new(ctx.scale, 0.0, 0.0, ctx.scale, 0.0, 0.0));
        }

        context.mask(&pattern)?;
    }

    context.restore()?;

    Ok(())
}
