use crate::render::svg_repo::SvgRepoError;
use std::fmt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LayerRenderError {
    #[error("DB error: {0}")]
    PostgresError(#[from] PostgresRenderError),

    #[error("Cairo error: {0}")]
    CairoError(#[from] cairo::Error),

    #[error("Error getting SVG: {0}")]
    SvgError(#[from] SvgRepoError),

    #[error("Invalid GeoJSON: {0}")]
    GeoJsonError(#[from] geojson::Error),

    #[error("GDAL error: {0}")]
    GdalError(#[from] gdal::errors::GdalError),

    #[error("Cairo borrow error: {0}")]
    CairoBorrowError(#[from] cairo::BorrowError),
}

pub type LayerRenderResult = Result<(), LayerRenderError>;

#[derive(Debug)]
pub struct PostgresRenderError(pub postgres::Error);

impl fmt::Display for PostgresRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(db_err) = self.0.as_db_error() {
            write!(f, "{}: {}", db_err.code().code(), db_err.message())?;

            if let Some(detail) = db_err.detail() {
                write!(f, " | detail: {detail}")?;
            }

            if let Some(hint) = db_err.hint() {
                write!(f, " | hint: {hint}")?;
            }

            if let Some(position) = db_err.position() {
                write!(f, " | position: {position:?}")?;
            }

            Ok(())
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl std::error::Error for PostgresRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<postgres::Error> for PostgresRenderError {
    fn from(err: postgres::Error) -> Self {
        Self(err)
    }
}

impl From<postgres::Error> for LayerRenderError {
    fn from(err: postgres::Error) -> Self {
        Self::PostgresError(err.into())
    }
}
