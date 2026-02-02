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

impl From<&[u8]> for TileCoord {
    fn from(value: &[u8]) -> Self {
        let z = value.len();

        assert!(z <= 32);

        let (mut x, mut y) = (0u32, 0u32);

        for &d in value {
            assert!(d <= 3);

            x = (x << 1) | u32::from(d & 1);

            y = (y << 1) | u32::from((d >> 1) & 1);
        }

        Self {
            zoom: z as u8,

            x,

            y,
        }
    }
}

impl From<TileCoord> for Vec<u8> {
    fn from(t: TileCoord) -> Self {
        let z = t.zoom as usize;

        assert!(z <= 32);

        assert!(z == 0 || t.x < (1u32 << z));

        assert!(z == 0 || t.y < (1u32 << z));

        let mut out = Vec::with_capacity(z);

        for level in 0..z {
            let bit = (z - 1 - level) as u32;

            let bx = ((t.x >> bit) & 1) as u8;

            let by = ((t.y >> bit) & 1) as u8;

            out.push((by << 1) | bx);
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_empty_is_z0() {
        let t = TileCoord::from(&[] as &[u8]);

        assert_eq!(t.zoom, 0);

        assert_eq!(t.x, 0);

        assert_eq!(t.y, 0);
    }

    #[test]
    fn quadrants_z1() {
        assert_eq!(
            TileCoord::from([0u8].as_slice()),
            TileCoord {
                zoom: 1,
                x: 0,
                y: 0
            }
        );

        assert_eq!(
            TileCoord::from([1u8].as_slice()),
            TileCoord {
                zoom: 1,
                x: 1,
                y: 0
            }
        );

        assert_eq!(
            TileCoord::from([2u8].as_slice()),
            TileCoord {
                zoom: 1,
                x: 0,
                y: 1
            }
        );

        assert_eq!(
            TileCoord::from([3u8].as_slice()),
            TileCoord {
                zoom: 1,
                x: 1,
                y: 1
            }
        );
    }

    #[test]
    fn key_example_5_10_20() {
        let t = TileCoord {
            zoom: 5,
            x: 10,
            y: 20,
        };

        let k: Vec<u8> = t.into();

        assert_eq!(k, vec![2, 1, 2, 1, 0]);
    }

    #[test]
    fn roundtrip_some_cases() {
        for t in [
            TileCoord {
                zoom: 0,
                x: 0,
                y: 0,
            },
            TileCoord {
                zoom: 1,
                x: 0,
                y: 0,
            },
            TileCoord {
                zoom: 1,
                x: 1,
                y: 1,
            },
            TileCoord {
                zoom: 5,
                x: 10,
                y: 20,
            },
            TileCoord {
                zoom: 20,
                x: (1u32 << 20) - 1,
                y: (1u32 << 20) - 1,
            },
            TileCoord {
                zoom: 20,
                x: 123_456,
                y: 654_321,
            },
        ] {
            let k: Vec<u8> = t.into();

            let decoded = TileCoord::from(k.as_slice());

            assert_eq!(decoded, t);
        }
    }
}
