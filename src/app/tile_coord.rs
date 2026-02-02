use std::{fmt::Display, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TileCoord {
    pub(crate) zoom: u8,
    pub(crate) x: u32,
    pub(crate) y: u32,
}

impl TileCoord {
    pub(crate) fn parent(self) -> Option<Self> {
        if self.zoom == 0 {
            return None;
        }

        Some(Self {
            zoom: self.zoom - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    pub(crate) fn ancestor_at_zoom(self, zoom: u8) -> Option<Self> {
        if self.zoom <= zoom {
            return None;
        }

        let shift = self.zoom - zoom;

        Some(Self {
            zoom,
            x: self.x >> shift,
            y: self.y >> shift,
        })
    }

    pub(crate) fn is_ancestor_of(self, other: Self) -> bool {
        if self.zoom > other.zoom {
            return false;
        }

        let shift = other.zoom - self.zoom;
        (other.x >> shift) == self.x && (other.y >> shift) == self.y
    }
}

impl Display for TileCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", self.zoom, self.x, self.y)
    }
}

impl FromStr for TileCoord {
    type Err = TileCoordParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut iter = s.split('/');
        let zoom = iter
            .next()
            .ok_or(TileCoordParseError::InvalidFormat)?
            .parse::<u8>()?;
        let x = iter
            .next()
            .ok_or(TileCoordParseError::InvalidFormat)?
            .parse::<u32>()?;
        let y = iter
            .next()
            .ok_or(TileCoordParseError::InvalidFormat)?
            .parse::<u32>()?;
        if iter.next().is_some() {
            return Err(TileCoordParseError::InvalidFormat);
        }
        Ok(Self { zoom, x, y })
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TileCoordParseError {
    #[error("invalid tile coordinate format")]
    InvalidFormat,
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),
    #[error(transparent)]
    ParseFloat(#[from] std::num::ParseFloatError),
}
