use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct HillshadingEntry {
    /// Pre-leaked country code, suitable for `&'static str` APIs (e.g. HashMap keys).
    pub country: &'static str,
    /// Pre-leaked better-country codes.
    pub better: Vec<&'static str>,
}

/// Per-country hillshading priority. Each entry is `country` or `country:better1,better2,…`
/// where `better*` are countries whose hillshading masks override this one.
#[derive(Clone, Debug)]
pub struct HillshadingHierarchy(Vec<HillshadingEntry>);

impl HillshadingHierarchy {
    pub fn entries(&self) -> &[HillshadingEntry] {
        &self.0
    }
}

impl FromStr for HillshadingHierarchy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut entries = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for raw in value.split(';') {
            let raw = raw.trim();

            if raw.is_empty() {
                return Err("hillshading-hierarchy entry cannot be empty".into());
            }

            let (country_str, better_strs) = match raw.split_once(':') {
                Some((c, b)) => {
                    let country = c.trim().to_string();
                    let better: Vec<String> = b.split(',').map(|x| x.trim().to_string()).collect();

                    if better.iter().any(|x| x.is_empty()) {
                        return Err(format!(
                            "empty better-country code in hillshading-hierarchy entry '{raw}'"
                        ));
                    }

                    (country, better)
                }
                None => (raw.to_string(), Vec::new()),
            };

            if country_str.is_empty() {
                return Err(format!(
                    "empty country code in hillshading-hierarchy entry '{raw}'"
                ));
            }

            if !seen.insert(country_str.clone()) {
                return Err(format!(
                    "duplicate country '{country_str}' in hillshading-hierarchy"
                ));
            }

            let country: &'static str = Box::leak(country_str.into_boxed_str());
            let better: Vec<&'static str> = better_strs
                .into_iter()
                .map(|s| -> &'static str { Box::leak(s.into_boxed_str()) })
                .collect();

            entries.push(HillshadingEntry { country, better });
        }

        if entries.is_empty() {
            return Err("hillshading-hierarchy cannot be empty".into());
        }

        Ok(Self(entries))
    }
}

#[derive(Clone, Debug)]
pub struct ContourEntry {
    /// Pre-leaked country code, suitable for `&'static str` APIs (e.g. closure captures).
    pub country: &'static str,
    /// Pre-leaked tracing identifier `contours_<lc>`.
    pub layer_name: &'static str,
}

/// Country contour sources. Comma-separated country codes; the token `_` enables
/// the global fallback source. Tracing identifier is derived as `contours_<lc>` /
/// `contours_fallback`.
#[derive(Clone, Debug)]
pub struct ContourCountries {
    countries: Vec<ContourEntry>,
    has_fallback: bool,
}

impl ContourCountries {
    pub fn entries(&self) -> &[ContourEntry] {
        &self.countries
    }

    pub fn has_fallback(&self) -> bool {
        self.has_fallback
    }
}

impl FromStr for ContourCountries {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut countries: Vec<ContourEntry> = Vec::new();
        let mut has_fallback = false;
        let mut seen: HashSet<String> = HashSet::new();

        for raw in value.split(',') {
            let token = raw.trim();

            if token.is_empty() {
                return Err("contour-countries entry cannot be empty".into());
            }

            if token == "_" {
                if has_fallback {
                    return Err("contour-countries fallback '_' may appear at most once".into());
                }

                has_fallback = true;

                continue;
            }

            if !seen.insert(token.to_string()) {
                return Err(format!("duplicate country '{token}' in contour-countries"));
            }

            countries.push(ContourEntry {
                country: Box::leak(token.to_string().into_boxed_str()),
                layer_name: Box::leak(format!("contours_{token}").into_boxed_str()),
            });
        }

        if countries.is_empty() && !has_fallback {
            return Err("contour-countries cannot be empty".into());
        }

        Ok(Self {
            countries,
            has_fallback,
        })
    }
}

/// Static, server-side render configuration that does not vary per request.
#[derive(Clone, Debug)]
pub struct RenderConfig {
    pub svg_base_path: Arc<Path>,
    pub hillshading_base_path: Option<PathBuf>,
    pub hillshading_hierarchy: Option<HillshadingHierarchy>,
    pub contour_countries: Option<ContourCountries>,
}
