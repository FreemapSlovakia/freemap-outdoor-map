use crate::render::{DEFAULT_JPEG_QUALITY, ImageFormat, Layers, RenderLayer};
use clap::ValueEnum as _;
use std::{collections::HashSet, path::PathBuf, str::FromStr};

/// One tile route: a URL prefix, what it draws, in what format, and where it
/// caches.
#[derive(Clone, Debug)]
pub struct TileVariant {
    pub url_path: String,
    pub layers: Layers,
    pub format: ImageFormat,
    pub tile_cache_base_path: Option<PathBuf>,
    pub tile_index: Option<PathBuf>,
    pub coverage_geojson: Option<PathBuf>,
}

/// Every tile route, parsed from one setting.
///
/// Entries are separated by `;` and may span lines. An entry is a URL path
/// followed by space-separated fields in any order:
///
/// - `jpeg[=<quality>]` / `png` / `webp` / `webp-lossy[=<quality>]` — output
///   format, `jpeg` by default; quality defaults to 90 and 80 respectively.
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
                format: ImageFormat::Jpeg(DEFAULT_JPEG_QUALITY),
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
                } else if let Some(format) = ImageFormat::parse(field) {
                    if format_seen {
                        return Err(format!("tile variant '{entry}' names two formats"));
                    }

                    format_seen = true;
                    variant.format =
                        format.map_err(|err| format!("tile variant '{entry}': {err}"))?;
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
            if !variant.layers.is_whole_map() && !variant.format.has_alpha() {
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
    use crate::render::{DEFAULT_WEBP_QUALITY, WebpQuality};

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
        assert_eq!(v[0].format, ImageFormat::Jpeg(DEFAULT_JPEG_QUALITY));
        assert!(v[0].layers.base_map);
        assert!(v[0].layers.draws(RenderLayer::Shading));
        assert_eq!(
            v[0].coverage_geojson.as_deref(),
            Some("/c.geojson".as_ref())
        );

        assert!(!v[1].layers.base_map);
        assert_eq!(v[1].format, ImageFormat::Webp(WebpQuality::Lossless));
        assert!(v[1].layers.draws(RenderLayer::SacScale));
        assert!(!v[1].layers.draws(RenderLayer::Sea));

        assert!(v[2].layers.base_map);
        assert!(!v[2].layers.draws(RenderLayer::Landcover));
        assert!(v[2].layers.draws(RenderLayer::Buildings));
    }

    #[test]
    fn reads_a_quality_off_the_format() {
        let v = parse(
            "/a jpeg=70; /b webp-lossy=5 overlay +sac-scale; /c webp-lossy overlay +sac-scale",
        )
        .expect("parses");

        assert_eq!(v[0].format, ImageFormat::Jpeg(70));
        assert_eq!(v[1].format, ImageFormat::Webp(WebpQuality::Lossy(5.0)));
        assert_eq!(
            v[2].format,
            ImageFormat::Webp(WebpQuality::Lossy(DEFAULT_WEBP_QUALITY))
        );
    }

    #[test]
    fn rejects_a_quality_that_is_not_one() {
        assert!(
            parse("/ jpeg=101")
                .expect_err("too high")
                .contains("outside 0..=100")
        );
        assert!(
            parse("/ jpeg=-1")
                .expect_err("negative")
                .contains("outside 0..=100")
        );
        assert!(
            parse("/ jpeg=abc")
                .expect_err("not a number")
                .contains("not a number")
        );
        assert!(
            parse("/o/x webp=80 overlay +sac-scale")
                .expect_err("lossless takes none")
                .contains("takes no quality")
        );
        assert!(
            parse("/ png=80")
                .expect_err("lossless takes none")
                .contains("takes no quality")
        );
    }

    #[test]
    fn rejects_a_layer_in_the_wrong_list() {
        assert!(
            parse("/ +landcover")
                .expect_err("base layer added")
                .contains("omit list")
        );
        assert!(
            parse("/ -contours")
                .expect_err("extra omitted")
                .contains("render list")
        );
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
        assert!(
            parse("/ ; /")
                .expect_err("duplicate path")
                .contains("duplicate")
        );
        assert!(
            parse("x jpeg")
                .expect_err("no slash")
                .contains("must start with '/'")
        );
        assert!(
            parse("/ wat")
                .expect_err("junk field")
                .contains("unknown field")
        );
        assert!(
            parse("/ jpeg webp")
                .expect_err("two formats")
                .contains("two formats")
        );
    }
}
