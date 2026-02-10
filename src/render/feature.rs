use std::collections::HashMap;

use geo::{Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use geo_postgis::FromPostgis;
use postgis::ewkb::GeometryT as EwkbGeometry;
use postgres::Row;

#[derive(Clone, Debug)]
pub enum LegendValue {
    String(String),
    Bool(bool),
    F64(f64),
    I16(i16),
    I32(i32),
    Hstore(HashMap<String, Option<String>>),
    Point(Point),
    LineString(LineString),
    Geometry(Geometry),
}

#[derive(thiserror::Error, Debug)]
#[error("wrong type for '{field}': expected {expected}, got {actual}")]
pub struct WrongTypeError {
    field: String,
    expected: &'static str,
    actual: &'static str,
}

impl WrongTypeError {
    pub fn new(field: impl Into<String>, expected: &'static str, actual: &'static str) -> Self {
        Self {
            field: field.into(),
            expected,
            actual,
        }
    }
}

fn legend_value_type(value: &LegendValue) -> &'static str {
    match value {
        LegendValue::String(_) => "String",
        LegendValue::Bool(_) => "Bool",
        LegendValue::F64(_) => "F64",
        LegendValue::I16(_) => "I16",
        LegendValue::I32(_) => "I32",
        LegendValue::Hstore(_) => "Hstore",
        LegendValue::Point(_) => "Point",
        LegendValue::LineString(_) => "LineString",
        LegendValue::Geometry(_) => "Geometry",
    }
}

#[derive(thiserror::Error, Debug)]
pub enum FeatureError {
    #[error("Wrong type error: {0}")]
    WrongTypeError(#[from] WrongTypeError),
    #[error("Geom error: {0}")]
    GeomError(#[from] GeomError),
    #[error("missing value for '{field}' (expected {expected})")]
    MissingValue {
        field: String,
        expected: &'static str,
    },
    #[error("Error getting value from database: {0}")]
    PgError(#[from] postgres::Error),
}

pub enum Feature {
    Row(Row),
    LegendData(HashMap<String, LegendValue>),
}

pub const GEOMETRY_COLUMN: &str = "geometry";

impl Feature {
    pub fn get_geometry(&self) -> Result<Geometry, FeatureError> {
        match self {
            Self::Row(row) => Ok(geometry_geometry(row)?),
            Self::LegendData(data) => {
                match data
                    .get(GEOMETRY_COLUMN)
                    .ok_or(FeatureError::MissingValue {
                        field: GEOMETRY_COLUMN.to_string(),
                        expected: "Geometry",
                    })? {
                    LegendValue::Geometry(geometry) => Ok(geometry.clone()),
                    other => Err(WrongTypeError::new(
                        GEOMETRY_COLUMN,
                        "Geometry",
                        legend_value_type(other),
                    )
                    .into()),
                }
            }
        }
    }

    pub fn get_line_string(&self) -> Result<LineString, FeatureError> {
        match self {
            Self::Row(row) => Ok(geometry_line_string(row)?),
            Self::LegendData(data) => {
                match data
                    .get(GEOMETRY_COLUMN)
                    .ok_or(FeatureError::MissingValue {
                        field: GEOMETRY_COLUMN.to_string(),
                        expected: "LineString",
                    })? {
                    LegendValue::LineString(line_string) => Ok(line_string.clone()),
                    LegendValue::Geometry(Geometry::LineString(line_string)) => {
                        Ok(line_string.clone())
                    }
                    other => Err(WrongTypeError::new(
                        GEOMETRY_COLUMN,
                        "LineString",
                        legend_value_type(other),
                    )
                    .into()),
                }
            }
        }
    }

    pub fn get_point(&self) -> Result<Point, FeatureError> {
        match self {
            Self::Row(row) => Ok(geometry_point(row)?),
            Self::LegendData(data) => {
                match data
                    .get(GEOMETRY_COLUMN)
                    .ok_or(FeatureError::MissingValue {
                        field: GEOMETRY_COLUMN.to_string(),
                        expected: "Point",
                    })? {
                    LegendValue::Point(point) => Ok(point.clone()),
                    LegendValue::Geometry(Geometry::Point(point)) => Ok(point.clone()),
                    other => {
                        Err(
                            WrongTypeError::new(GEOMETRY_COLUMN, "Point", legend_value_type(other))
                                .into(),
                        )
                    }
                }
            }
        }
    }

    pub(crate) fn get_string(&self, arg: &str) -> Result<&str, FeatureError> {
        match self {
            Self::Row(row) => Ok(row.try_get(arg)?),
            Self::LegendData(data) => match data.get(arg).ok_or(FeatureError::MissingValue {
                field: arg.to_string(),
                expected: "String",
            })? {
                LegendValue::String(string) => Ok(string.as_str()),
                other => Err(WrongTypeError::new(arg, "String", legend_value_type(other)).into()),
            },
        }
    }

    pub(crate) fn get_bool(&self, arg: &str) -> Result<bool, FeatureError> {
        match self {
            Self::Row(row) => Ok(row.try_get(arg)?),
            Self::LegendData(data) => match data.get(arg).ok_or(FeatureError::MissingValue {
                field: arg.to_string(),
                expected: "Bool",
            })? {
                LegendValue::Bool(value) => Ok(*value),
                other => Err(WrongTypeError::new(arg, "bool", legend_value_type(other)).into()),
            },
        }
    }

    pub(crate) fn get_f64(&self, arg: &str) -> Result<f64, FeatureError> {
        match self {
            Self::Row(row) => Ok(row.try_get(arg)?),
            Self::LegendData(data) => match data.get(arg).ok_or(FeatureError::MissingValue {
                field: arg.to_string(),
                expected: "F64",
            })? {
                LegendValue::F64(value) => Ok(*value),
                other => Err(WrongTypeError::new(arg, "f64", legend_value_type(other)).into()),
            },
        }
    }

    pub(crate) fn get_i16(&self, arg: &str) -> Result<i16, FeatureError> {
        match self {
            Self::Row(row) => Ok(row.try_get(arg)?),
            Self::LegendData(data) => match data.get(arg).ok_or(FeatureError::MissingValue {
                field: arg.to_string(),
                expected: "I16",
            })? {
                LegendValue::I16(value) => Ok(*value),
                other => Err(WrongTypeError::new(arg, "i16", legend_value_type(other)).into()),
            },
        }
    }

    pub(crate) fn get_i32(&self, arg: &str) -> Result<i32, FeatureError> {
        match self {
            Self::Row(row) => Ok(row.try_get(arg)?),
            Self::LegendData(data) => match data.get(arg).ok_or(FeatureError::MissingValue {
                field: arg.to_string(),
                expected: "I32",
            })? {
                LegendValue::I32(value) => Ok(*value),
                other => Err(WrongTypeError::new(arg, "i32", legend_value_type(other)).into()),
            },
        }
    }

    pub(crate) fn get_hstore(
        &self,
        arg: &str,
    ) -> Result<HashMap<String, Option<String>>, FeatureError> {
        match self {
            Self::Row(row) => Ok(row.try_get(arg)?),
            Self::LegendData(data) => {
                let value = data.get(arg).ok_or(FeatureError::MissingValue {
                    field: arg.to_string(),
                    expected: "Hstore",
                })?;
                match value {
                    LegendValue::Hstore(value) => Ok(value.clone()),
                    other => {
                        Err(WrongTypeError::new(arg, "Hstore", legend_value_type(other)).into())
                    }
                }
            }
        }
    }
}

impl From<Row> for Feature {
    fn from(value: Row) -> Self {
        Feature::Row(value)
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

fn geometry_point(row: &Row) -> Result<Point, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::Point(geom) => Ok(Point::from_postgis(&geom)),
        other => Err(GeomError::UnexpectedType {
            expected: "Point",
            got: geometry_type_name(&other),
        }),
    }
}

fn geometry_line_string(row: &Row) -> Result<LineString, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::LineString(geom) => Ok(LineString::from_postgis(&geom)),
        other => Err(GeomError::UnexpectedType {
            expected: "LineString",
            got: geometry_type_name(&other),
        }),
    }
}

#[allow(dead_code)]
fn geometry_multi_line_string(row: &Row) -> Result<MultiLineString, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::MultiLineString(geom) => Ok(MultiLineString::from_postgis(&geom)),
        other => Err(GeomError::UnexpectedType {
            expected: "MultiLineString",
            got: geometry_type_name(&other),
        }),
    }
}

#[allow(dead_code)]
fn geometry_polygon(row: &Row) -> Result<Polygon, GeomError> {
    match row.try_get::<_, EwkbGeometry<_>>(GEOMETRY_COLUMN)? {
        EwkbGeometry::Polygon(geom) => Option::from_postgis(&geom).ok_or(GeomError::GeomIsEmpty),
        other => Err(GeomError::UnexpectedType {
            expected: "Polygon",
            got: geometry_type_name(&other),
        }),
    }
}

fn geometry_geometry(row: &Row) -> Result<Geometry, GeomError> {
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
            geo::GeometryCollection::from_postgis(&geom),
        )),
    }
}
