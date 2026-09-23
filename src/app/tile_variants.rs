use crate::render::{ImageFormat, Layers, RenderLayer, WebpQuality};
use clap::ValueEnum;
use std::{collections::HashSet, path::PathBuf, str::FromStr};

/// One tile route: a URL prefix, what it draws, in what format, and where it
/// caches.
#[derive(Clone, Debug)]
pub struct TileVariant {
    pub url_path: String,
    pub layers: Layers,
    pub format: TileFormat,
    pub tile_cache_base_path: Option<PathBuf>,
    pub tile_index: Option<PathBuf>,
    pub coverage_geojson: Option<PathBuf>,
}

/// A variant's output format. Lossy WebP only pays on an overlay dense enough
/// that lossless has no sparsity to exploit; a sparse one encodes smaller
/// lossless, and without the fringing a photo codec leaves around text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum TileFormat {
    #[default]
    Jpeg,
    Png,
    Webp,
    WebpLossy,
}

impl TileFormat {
    pub const fn image_format(self, webp_quality: f32) -> ImageFormat {
        match self {
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Png => ImageFormat::Png,
            Self::Webp => ImageFormat::Webp(WebpQuality::Lossless),
            Self::WebpLossy => ImageFormat::Webp(WebpQuality::Lossy(webp_quality)),
        }
    }

    fn parse(token: &str) -> Option<Self> {
        Some(match token {
            "jpeg" => Self::Jpeg,
            "png" => Self::Png,
            "webp" => Self::Webp,
            "webp-lossy" => Self::WebpLossy,
            _ => return None,
        })
    }
}

/// Every tile route, parsed from one setting.
///
/// Entries are separated by `;` and may span lines. An entry is a URL path
/// followed by space-separated fields in any order:
///
/// - `jpeg` / `png` / `webp` / `webp-lossy` — output format, `jpeg` by default.
/// - `overlay` — do not draw the layers the map draws by itself.
/// - `+<layer>[,<layer>…]` — extras to draw.
/// - `-<layer>[,<layer>…]` — base layers to drop.
/// - `cache=<dir>`, `index=<dir>`, `coverage=<file>`.
///
/// ```text
/// /         jpeg       +shading,contours,routes-hiking  cache=/tiles/map;
/// /o/sac    webp       overlay +sac-scale               cache=/tiles/sac;
/// /o/aerial webp-lossy -landcover,sea,buildings         cache=/tiles/aerial
/// ```
#[derive(Clone, Debug)]
pub struct TileVariants(Vec<TileVariant>);

impl TileVariants {
    pub fn entries(&self) -> &[TileVariant] {
        &self.0
    }
}

fn parse_layers(list: &str, entry: &str) -> Result<HashSet<RenderLayer>, String> {
    list.split(',')
        .map(|name| {
            let name = name.trim();

            RenderLayer::from_str(name, true)
                .map_err(|_| format!("unknown render layer '{name}' in tile variant '{entry}'"))
        })
        .collect()
}

impl FromStr for TileVariants {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut variants = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for entry in value.split(';') {
            let entry = entry.trim();

            if entry.is_empty() {
                continue;
            }

            let mut fields = entry.split_whitespace();

            let url_path = fields
                .next()
                .ok_or_else(|| format!("tile variant '{entry}' has no URL path"))?;

            if !url_path.starts_with('/') {
                return Err(format!(
                    "tile variant URL path '{url_path}' must start with '/'"
                ));
            }

            let url_path = if url_path == "/" {
                url_path.to_owned()
            } else {
                url_path.trim_end_matches('/').to_owned()
            };

            if !seen.insert(url_path.clone()) {
                return Err(format!("duplicate tile variant URL path '{url_path}'"));
            }

            let mut variant = TileVariant {
                url_path,
                layers: Layers {
                    base_map: true,
                    add: HashSet::new(),
                    omit: HashSet::new(),
                },
                format: TileFormat::default(),
                tile_cache_base_path: None,
                tile_index: None,
                coverage_geojson: None,
            };

            let mut format_seen = false;

            for field in fields {
                if let Some(add) = field.strip_prefix('+') {
                    variant.layers.add.extend(parse_layers(add, entry)?);
                } else if let Some(omit) = field.strip_prefix('-') {
                    variant.layers.omit.extend(parse_layers(omit, entry)?);
                } else if field == "overlay" {
                    variant.layers.base_map = false;
                } else if let Some(path) = field.strip_prefix("cache=") {
                    variant.tile_cache_base_path = Some(PathBuf::from(path));
                } else if let Some(path) = field.strip_prefix("index=") {
                    variant.tile_index = Some(PathBuf::from(path));
                } else if let Some(path) = field.strip_prefix("coverage=") {
                    variant.coverage_geojson = Some(PathBuf::from(path));
                } else if let Some(format) = TileFormat::parse(field) {
                    if format_seen {
                        return Err(format!("tile variant '{entry}' names two formats"));
                    }

                    format_seen = true;
                    variant.format = format;
                } else {
                    return Err(format!("unknown field '{field}' in tile variant '{entry}'"));
                }
            }

            variant
                .layers
                .validate()
                .map_err(|err| format!("tile variant '{}': {err}", variant.url_path))?;

            // An overlay leaves most of the surface unpainted; an opaque format
            // renders that as solid black rather than as nothing.
            if !variant.layers.is_whole_map()
                && !variant.format.image_format(1.0).has_alpha()
            {
                return Err(format!(
                    "tile variant '{}' is an overlay, so it needs an alpha-capable format",
                    variant.url_path
                ));
            }

            variants.push(variant);
        }

        if variants.is_empty() {
            return Err("at least one tile variant is required".into());
        }

        Ok(Self(variants))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Vec<TileVariant>, String> {
        TileVariants::from_str(s).map(|v| v.0)
    }

    #[test]
    fn reads_a_map_and_two_overlays() {
        let v = parse(
            "
            /         +shading,contours cache=/t/map coverage=/c.geojson;
            /o/sac    webp overlay +sac-scale cache=/t/sac;
            /o/aerial webp-lossy -landcover,sea cache=/t/aerial
            ",
        )
        .expect("parses");

        assert_eq!(v.len(), 3);

        assert_eq!(v[0].url_path, "/");
        assert_eq!(v[0].format, TileFormat::Jpeg);
        assert!(v[0].layers.base_map);
        assert!(v[0].layers.draws(RenderLayer::Shading));
        assert_eq!(v[0].coverage_geojson.as_deref(), Some("/c.geojson".as_ref()));

        assert!(!v[1].layers.base_map);
        assert_eq!(v[1].format, TileFormat::Webp);
        assert!(v[1].layers.draws(RenderLayer::SacScale));
        assert!(!v[1].layers.draws(RenderLayer::Sea));

        assert!(v[2].layers.base_map);
        assert!(!v[2].layers.draws(RenderLayer::Landcover));
        assert!(v[2].layers.draws(RenderLayer::Buildings));
    }

    #[test]
    fn rejects_a_layer_in_the_wrong_list() {
        assert!(parse("/ +landcover").expect_err("base layer added").contains("omit list"));
        assert!(parse("/ -contours").expect_err("extra omitted").contains("render list"));
    }

    #[test]
    fn rejects_an_opaque_overlay() {
        assert!(
            parse("/o/x jpeg overlay +sac-scale")
                .expect_err("opaque overlay")
                .contains("alpha-capable")
        );
    }

    #[test]
    fn rejects_duplicates_and_junk() {
        assert!(parse("/ ; /").expect_err("duplicate path").contains("duplicate"));
        assert!(parse("x jpeg").expect_err("no slash").contains("must start with '/'"));
        assert!(parse("/ wat").expect_err("junk field").contains("unknown field"));
        assert!(parse("/ jpeg webp").expect_err("two formats").contains("two formats"));
    }
}
