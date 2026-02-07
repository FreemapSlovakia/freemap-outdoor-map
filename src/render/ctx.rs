use crate::render::{
    projectable::{
        GeomError, TileProjector, geometry_geometry, geometry_line_string, geometry_point,
    },
    size::Size,
};
use cairo::Context;
use geo::{Geometry, LineString, Point, Rect};
use postgres::{Row, types::ToSql};
use std::collections::HashMap;

pub struct SqlParams {
    params: Vec<Box<dyn ToSql + Sync>>,
}

impl SqlParams {
    pub fn push<T>(mut self, value: T) -> Self
    where
        T: ToSql + Sync + 'static,
    {
        self.params.push(Box::new(value));

        self
    }

    pub fn as_params(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params.iter().map(|param| param.as_ref()).collect()
    }
}

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
    fn new(field: impl Into<String>, expected: &'static str, actual: &'static str) -> Self {
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

const GEOMETRY_COLUMN: &str = "geometry";

impl Feature {
    pub fn geometry(&self) -> Result<Geometry, FeatureError> {
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

    pub fn line_string(&self) -> Result<LineString, FeatureError> {
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

    pub fn point(&self) -> Result<Point, FeatureError> {
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

pub struct Ctx<'a> {
    pub context: &'a Context,
    pub bbox: Rect<f64>,
    pub size: Size<u32>,
    pub zoom: u8,
    pub tile_projector: TileProjector,
    pub scale: f64,
    pub legend: Option<&'a HashMap<String, Vec<HashMap<String, LegendValue>>>>,
}

impl Ctx<'_> {
    pub fn meters_per_pixel(&self) -> f64 {
        self.bbox.width() / self.size.width as f64
    }

    pub fn bbox_query_params(&self, buffer_from_param: Option<f64>) -> SqlParams {
        let min = self.bbox.min();
        let max = self.bbox.max();

        let mut params: Vec<Box<dyn ToSql + Sync>> = vec![
            Box::new(min.x),
            Box::new(min.y),
            Box::new(max.x),
            Box::new(max.y),
        ];

        if let Some(buffer_from_param) = buffer_from_param {
            params.push(Box::new(self.meters_per_pixel() * buffer_from_param));
        }

        SqlParams { params }
    }

    pub(crate) fn hint(&self, x: f64) -> f64 {
        (x * self.scale).round() / self.scale
    }

    pub fn legend_features(
        &self,
        layer_name: &str,
        mut cb: impl FnMut() -> Result<Vec<Row>, postgres::Error>,
    ) -> Result<Vec<Feature>, postgres::Error> {
        let Some(ref legend) = self.legend else {
            return Ok(cb()?.into_iter().map(|row| row.into()).collect());
        };

        let Some(legend) = legend.get(layer_name) else {
            return Ok(vec![]);
        };

        Ok(legend
            .iter()
            .map(|props| Feature::LegendData(props.clone()))
            .collect())
    }
}
