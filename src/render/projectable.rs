use crate::render::size::Size;
use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};

pub struct TileProjector {
    min_x: f64,
    min_y: f64,
    scale_x: f64,
    scale_y: f64,
    height: f64,
}

impl TileProjector {
    pub fn new(bbox: Rect<f64>, size: Size<u32>) -> Self {
        let min = bbox.min();

        Self {
            min_x: min.x,
            min_y: min.y,
            scale_x: size.width as f64 / bbox.width(),
            scale_y: size.height as f64 / bbox.height(),
            height: size.height as f64,
        }
    }

    #[inline]
    pub fn project_coord(&self, coord: &Coord) -> Coord {
        Coord {
            x: (coord.x - self.min_x) * self.scale_x,
            y: (coord.y - self.min_y).mul_add(-self.scale_y, self.height),
        }
    }
}

pub trait TileProjectable {
    fn project_to_tile(&self, tp: &TileProjector) -> Self;
}

impl TileProjectable for Point {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self(tp.project_coord(&self.0))
    }
}

impl TileProjectable for LineString {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self::new(self.0.iter().map(|c| tp.project_coord(c)).collect())
    }
}

impl TileProjectable for Line {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self::new(tp.project_coord(&self.start), tp.project_coord(&self.end))
    }
}

impl TileProjectable for Polygon {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self::new(
            self.exterior().project_to_tile(tp),
            self.interiors()
                .iter()
                .map(|ls| ls.project_to_tile(tp))
                .collect(),
        )
    }
}

impl TileProjectable for MultiPoint {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self(self.0.iter().map(|p| p.project_to_tile(tp)).collect())
    }
}

impl TileProjectable for MultiLineString {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self(self.0.iter().map(|ls| ls.project_to_tile(tp)).collect())
    }
}

impl TileProjectable for MultiPolygon {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self(self.0.iter().map(|p| p.project_to_tile(tp)).collect())
    }
}

impl TileProjectable for GeometryCollection {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self(self.iter().map(|g| g.project_to_tile(tp)).collect())
    }
}

impl TileProjectable for Rect {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self::new(tp.project_coord(&self.min()), tp.project_coord(&self.max()))
    }
}

impl TileProjectable for Triangle {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        Self::new(
            tp.project_coord(&self.0),
            tp.project_coord(&self.1),
            tp.project_coord(&self.2),
        )
    }
}

impl TileProjectable for Geometry {
    fn project_to_tile(&self, tp: &TileProjector) -> Self {
        match self {
            Self::Point(p) => Self::Point(p.project_to_tile(tp)),
            Self::Line(l) => Self::Line(l.project_to_tile(tp)),
            Self::LineString(ls) => Self::LineString(ls.project_to_tile(tp)),
            Self::Polygon(p) => Self::Polygon(p.project_to_tile(tp)),
            Self::MultiPoint(mp) => Self::MultiPoint(mp.project_to_tile(tp)),
            Self::MultiLineString(mls) => Self::MultiLineString(mls.project_to_tile(tp)),
            Self::MultiPolygon(mp) => Self::MultiPolygon(mp.project_to_tile(tp)),
            Self::GeometryCollection(gc) => Self::GeometryCollection(gc.project_to_tile(tp)),
            Self::Rect(r) => Self::Rect(r.project_to_tile(tp)),
            Self::Triangle(t) => Self::Triangle(t.project_to_tile(tp)),
        }
    }
}
