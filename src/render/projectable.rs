use crate::render::size::Size;
use geo::{
    Coord, Geometry, GeometryCollection, Line, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect, Triangle,
};
use geo_postgis::FromPostgis;
use postgis::ewkb::GeometryT as EwkbGeometry;
use postgres::Row;

const GEOMETRY_COLUMN: &str = "geometry";

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

#[derive(thiserror::Error, Debug)]
pub enum GeomError {
    #[error("Error getting geometry from database: {0}")]
    PgError(#[from] postgres::Error),
    #[error("Empty or null geometry")]
    GeomIsEmpty,
    #[error("Unexpected geometry type: expected {expected}, got {got}")]
    UnexpectedType {
        expected: &'static str,
        got: &'static str,
    },
}

fn geometry_type_name(geometry: &EwkbGeometry<postgis::ewkb::Point>) -> &'static str {
    match geometry {
        EwkbGeometry::Point(_) => "Point",
        EwkbGeometry::LineString(_) => "LineString",
        EwkbGeometry::Polygon(_) => "Polygon",
        EwkbGeometry::MultiPoint(_) => "MultiPoint",
        EwkbGeometry::MultiLineString(_) => "MultiLineString",
        EwkbGeometry::MultiPolygon(_) => "MultiPolygon",
        EwkbGeometry::GeometryCollection(_) => "GeometryCollection",
    }
}

pub fn geometry_point(row: &Row) -> Result<Point, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::Point(geom) => Ok(Point::from_postgis(&geom)),
        other => Err(GeomError::UnexpectedType {
            expected: "Point",
            got: geometry_type_name(&other),
        }),
    }
}

pub fn geometry_line_string(row: &Row) -> Result<LineString, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::LineString(geom) => Ok(LineString::from_postgis(&geom)),
        other => Err(GeomError::UnexpectedType {
            expected: "LineString",
            got: geometry_type_name(&other),
        }),
    }
}

#[allow(dead_code)]
pub fn geometry_multi_line_string(row: &Row) -> Result<MultiLineString, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::MultiLineString(geom) => Ok(MultiLineString::from_postgis(&geom)),
        other => Err(GeomError::UnexpectedType {
            expected: "MultiLineString",
            got: geometry_type_name(&other),
        }),
    }
}

#[allow(dead_code)]
pub fn geometry_polygon(row: &Row) -> Result<Polygon, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::Polygon(geom) => Option::from_postgis(&geom).ok_or(GeomError::GeomIsEmpty),
        other => Err(GeomError::UnexpectedType {
            expected: "Polygon",
            got: geometry_type_name(&other),
        }),
    }
}

pub fn geometry_geometry(row: &Row) -> Result<Geometry, GeomError> {
    match row.try_get::<_, EwkbGeometry<postgis::ewkb::Point>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::Point(geom) => Ok(Geometry::Point(Point::from_postgis(&geom))),
        EwkbGeometry::LineString(geom) => Ok(Geometry::LineString(LineString::from_postgis(&geom))),
        EwkbGeometry::Polygon(geom) => Ok(Geometry::Polygon(
            Option::from_postgis(&geom).ok_or(GeomError::GeomIsEmpty)?,
        )),
        EwkbGeometry::MultiPoint(geom) => Ok(Geometry::MultiPoint(MultiPoint::from_postgis(&geom))),
        EwkbGeometry::MultiLineString(geom) => Ok(Geometry::MultiLineString(
            MultiLineString::from_postgis(&geom),
        )),
        EwkbGeometry::MultiPolygon(geom) => {
            Ok(Geometry::MultiPolygon(MultiPolygon::from_postgis(&geom)))
        }
        EwkbGeometry::GeometryCollection(geom) => Ok(Geometry::GeometryCollection(
            GeometryCollection::from_postgis(&geom),
        )),
    }
}
