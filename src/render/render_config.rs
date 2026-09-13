use crate::render::layers::place_names::{
    LABEL_STYLES, LabelStyle, MAX_PLACE_ZOOM, label_style, place_type_default, place_type_min_zoom,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct HillshadingEntry {
    /// Pre-leaked country code, suitable for `&'static str` APIs (e.g. `HashMap` keys).
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

                    if better.iter().any(std::string::String::is_empty) {
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

    pub const fn has_fallback(&self) -> bool {
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

/// Countries whose hillshading is detailed enough to convey terrain feature lines
/// (cliffs, embankments, …) on its own; those lines are masked out where the country's
/// hillshading mask covers the tile. Comma-separated country codes.
#[derive(Clone, Debug)]
pub struct FeatureLineMaskCountries(Vec<String>);

impl FeatureLineMaskCountries {
    pub fn countries(&self) -> &[String] {
        &self.0
    }
}

impl FromStr for FeatureLineMaskCountries {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut countries: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for raw in value.split(',') {
            let token = raw.trim();

            if token.is_empty() {
                return Err("feature-line-mask-countries entry cannot be empty".into());
            }

            if !seen.insert(token.to_string()) {
                return Err(format!(
                    "duplicate country '{token}' in feature-line-mask-countries"
                ));
            }

            countries.push(token.to_string());
        }

        if countries.is_empty() {
            return Err("feature-line-mask-countries cannot be empty".into());
        }

        Ok(Self(countries))
    }
}

/// What a country override does to a place type.
#[derive(Clone, Copy, Debug)]
pub enum PlaceTypeOverride {
    /// Label the place from this zoom, in this style, instead of its type's default pair.
    RenderAs {
        min_zoom: u8,
        style: &'static LabelStyle,
    },
    /// Don't label the place at all.
    Hide,
}

/// Stands for every country without rules of its own, and for places outside any country.
pub const DEFAULT_COUNTRY: &str = "*";

/// One rule: what to do with a place type, optionally only below a population.
#[derive(Clone, Copy, Debug)]
struct PlaceTypeRule {
    /// The rule applies to places whose population is below this. `u32::MAX` for a rule
    /// without a population condition; an untagged population counts as 0.
    below_population: u32,
    override_: PlaceTypeOverride,
}

/// Per-country place type remapping. Countries whose `place=*` tagging is finer-grained
/// than ours (Croatian `place=village` for a 150-inhabitant naselje, Polish `place=hamlet`
/// for what is a part of a village elsewhere) can render a type as a lesser one, which
/// both postpones the label to that type's minimum zoom and shrinks it.
///
/// Format: `<country>:<rule>[,<rule>…][;<country>:…]`, where the country is a lowercase
/// ISO 3166-1 alpha-2 code or `*` for every country without rules of its own, and `<rule>`
/// is `<from>[@<population>]=<to>`. The target `<to>` is `z<zoom>[/<style>]` — from which
/// zoom to label the place, in which of the named `LABEL_STYLES`, defaulting to the one
/// the source type already uses — or `-` to not label the type at all. The optional
/// `@<population>` matches only places below that population; an untagged population
/// counts as 0, so it is demoted too. Several rules for one type form tiers: the one with
/// the lowest matching population wins, and a rule without `@` applies to whatever is left.
///
/// A remapping may only postpone a type, never make it appear earlier: the SQL query
/// filters by the original type, so an earlier type would not even be fetched.
#[derive(Clone, Debug)]
pub struct PlaceTypeOverrides(HashMap<&'static str, HashMap<&'static str, Vec<PlaceTypeRule>>>);

impl PlaceTypeOverrides {
    /// The override for a place, or `None` when no rule matches and it renders as tagged.
    pub fn get(
        &self,
        country: &str,
        place_type: &str,
        population: i32,
    ) -> Option<PlaceTypeOverride> {
        let population = u32::try_from(population).unwrap_or(0);

        // A country's own rules for a type replace the `*` ones rather than adding to them,
        // which is how a country opts out of a default it does not want.
        let tiers = self
            .0
            .get(country)
            .and_then(|rules| rules.get(place_type))
            .or_else(|| self.0.get(DEFAULT_COUNTRY)?.get(place_type))?;

        tiers
            .iter()
            .find(|rule| population < rule.below_population)
            .map(|rule| rule.override_)
    }
}

/// A rule target: `z<zoom>[/<style>]`. Without a style the source type keeps its own.
fn parse_target(rule: &str, from: &str, to: &str) -> Result<(u8, &'static LabelStyle), String> {
    let Some(zoom) = to.strip_prefix('z') else {
        return Err(format!(
            "place-type-overrides rule '{rule}' has target '{to}'; write it as 'z<zoom>[/<style>]' or '-'"
        ));
    };

    let (zoom, style) = match zoom.split_once('/') {
        None => (zoom, None),
        Some((zoom, style)) => (zoom, Some(style.trim())),
    };

    let Ok(zoom) = zoom.trim().parse::<u8>() else {
        return Err(format!(
            "place-type-overrides rule '{rule}' has an invalid zoom '{zoom}'"
        ));
    };

    let style = match style {
        Some(name) => label_style(name).ok_or_else(|| {
            let known = LABEL_STYLES
                .iter()
                .map(|style| style.name)
                .collect::<Vec<_>>()
                .join(", ");

            format!("place-type-overrides rule '{rule}' uses unknown style '{name}'; known styles are {known}")
        })?,
        None => place_type_default(from).map(|(_, style)| style).ok_or_else(|| {
            format!("place-type-overrides rule '{rule}' has no style and '{from}' has no default one; write it as 'z{zoom}/<style>'")
        })?,
    };

    Ok((zoom, style))
}

impl FromStr for PlaceTypeOverrides {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut countries: HashMap<&'static str, HashMap<&'static str, Vec<PlaceTypeRule>>> =
            HashMap::new();

        for raw in value.split(';') {
            let raw = raw.trim();

            if raw.is_empty() {
                return Err("place-type-overrides entry cannot be empty".into());
            }

            let Some((country, rules)) = raw.split_once(':') else {
                return Err(format!(
                    "place-type-overrides entry '{raw}' is missing ':<from>=<to>'"
                ));
            };

            let country = country.trim().to_lowercase();

            if country.is_empty() {
                return Err(format!(
                    "empty country code in place-type-overrides entry '{raw}'"
                ));
            }

            if countries.contains_key(country.as_str()) {
                return Err(format!(
                    "duplicate country '{country}' in place-type-overrides"
                ));
            }

            let mut parsed: HashMap<&'static str, Vec<PlaceTypeRule>> = HashMap::new();

            for rule in rules.split(',') {
                let rule = rule.trim();

                let Some((from, to)) = rule.split_once('=') else {
                    return Err(format!(
                        "place-type-overrides rule '{rule}' is not '<from>[@<population>]=<to>'"
                    ));
                };

                let to = to.trim();

                let (from, below_population) = match from.split_once('@') {
                    None => (from.trim(), u32::MAX),
                    Some((from, population)) => {
                        let population = population.trim();

                        let Ok(population) = population.parse::<u32>() else {
                            return Err(format!(
                                "place-type-overrides rule '{rule}' has an invalid population '{population}'"
                            ));
                        };

                        (from.trim(), population)
                    }
                };

                let Some(from_min_zoom) = place_type_min_zoom(from) else {
                    return Err(format!(
                        "place-type-overrides rule '{rule}' remaps unknown place type '{from}'"
                    ));
                };

                let override_ = if to == "-" {
                    PlaceTypeOverride::Hide
                } else {
                    let (to_min_zoom, style) = parse_target(rule, from, to)?;

                    if to_min_zoom < from_min_zoom {
                        return Err(format!(
                            "place-type-overrides rule '{rule}' would make '{from}' appear earlier (z{to_min_zoom}) than by default (z{from_min_zoom}); only postponing is supported"
                        ));
                    }

                    if to_min_zoom > MAX_PLACE_ZOOM {
                        return Err(format!(
                            "place-type-overrides rule '{rule}' postpones '{from}' past z{MAX_PLACE_ZOOM}, the last zoom with place labels; use '-' to drop the type instead"
                        ));
                    }

                    PlaceTypeOverride::RenderAs {
                        min_zoom: to_min_zoom,
                        style,
                    }
                };

                let tiers = parsed
                    .entry(Box::leak(from.to_string().into_boxed_str()))
                    .or_default();

                if tiers
                    .iter()
                    .any(|tier| tier.below_population == below_population)
                {
                    return Err(format!(
                        "duplicate rule for place type '{from}' of country '{country}' in place-type-overrides"
                    ));
                }

                tiers.push(PlaceTypeRule {
                    below_population,
                    override_,
                });
            }

            // Lowest population first, so `get` can take the first matching tier.
            for tiers in parsed.values_mut() {
                tiers.sort_by_key(|tier| tier.below_population);
            }

            countries.insert(Box::leak(country.into_boxed_str()), parsed);
        }

        if countries.is_empty() {
            return Err("place-type-overrides cannot be empty".into());
        }

        Ok(Self(countries))
    }
}

#[cfg(test)]
mod place_type_overrides_tests {
    use super::{PlaceTypeOverride, PlaceTypeOverrides};
    use std::str::FromStr;

    fn describe(override_: PlaceTypeOverride) -> String {
        match override_ {
            PlaceTypeOverride::RenderAs { min_zoom, style } => {
                format!("z{min_zoom}/{}", style.name)
            }
            PlaceTypeOverride::Hide => "-".to_owned(),
        }
    }

    /// The resolved rule as `z<zoom>/<style>`, `-` when hidden, or `default` when untouched.
    fn resolved(overrides: &PlaceTypeOverrides, place_type: &str, population: i32) -> String {
        overrides
            .get("hr", place_type, population)
            .map_or_else(|| "default".to_owned(), describe)
    }

    #[test]
    fn population_limits_a_rule_to_places_below_it() {
        let o =
            PlaceTypeOverrides::from_str("hr:village@100=z12/m,hamlet=z16/xxs").expect("parsed");

        assert_eq!(resolved(&o, "village", 0), "z12/m");
        assert_eq!(resolved(&o, "village", 99), "z12/m");
        assert_eq!(resolved(&o, "village", 100), "default");
        assert_eq!(resolved(&o, "village", 5000), "default");
        // No population condition, so every hamlet is demoted.
        assert_eq!(resolved(&o, "hamlet", 5000), "z16/xxs");
        // Other countries and untouched types are unaffected.
        assert!(o.get("sk", "village", 0).is_none());
        assert!(o.get("hr", "town", 0).is_none());
    }

    #[test]
    fn lowest_matching_population_wins() {
        let o = PlaceTypeOverrides::from_str("hr:village@500=z12/m,village@50=-").expect("parsed");

        assert_eq!(resolved(&o, "village", 30), "-");
        assert_eq!(resolved(&o, "village", 200), "z12/m");
        assert_eq!(resolved(&o, "village", 900), "default");
    }

    #[test]
    fn the_default_country_covers_the_rest() {
        let o = PlaceTypeOverrides::from_str("*:hamlet=z14/s,village@50=-;hr:hamlet=z16/xxs")
            .expect("parsed");

        // A country without rules of its own follows `*` …
        assert_eq!(o.get("sk", "hamlet", 0).map(describe), Some("z14/s".into()));
        // … as do places that fall outside every country polygon.
        assert_eq!(o.get("", "hamlet", 0).map(describe), Some("z14/s".into()));
        // A country's own rule for the type replaces the default rather than adding to it.
        assert_eq!(resolved(&o, "hamlet", 0), "z16/xxs");
        // Types the country does not mention still follow `*`.
        assert_eq!(resolved(&o, "village", 10), "-");
        assert_eq!(resolved(&o, "village", 60), "default");
    }

    #[test]
    fn zoom_and_style_can_be_given_directly() {
        let o = PlaceTypeOverrides::from_str("hr:village@300=z13/xs,hamlet=z13").expect("parsed");

        // Zooms the type ladder cannot express, and a style unrelated to any place type.
        assert_eq!(resolved(&o, "village", 100), "z13/xs");
        // Without a style the source type keeps its own.
        assert_eq!(resolved(&o, "hamlet", 0), "z13/m");
    }

    #[test]
    fn rejects_rules_that_would_show_a_type_earlier() {
        assert!(PlaceTypeOverrides::from_str("hr:village@100=town").is_err());
        assert!(PlaceTypeOverrides::from_str("hr:village@100=z10").is_err());
        assert!(PlaceTypeOverrides::from_str("hr:village@x=hamlet").is_err());
        assert!(PlaceTypeOverrides::from_str("hr:village@100=hamlet,village@100=farm").is_err());
    }

    #[test]
    fn rejects_unusable_targets() {
        // island/islet are drawn by area, so they name no zoom/style pair to borrow.
        assert!(PlaceTypeOverrides::from_str("hr:village=island").is_err());
        assert!(PlaceTypeOverrides::from_str("hr:village=hamlet").is_err());
        assert!(PlaceTypeOverrides::from_str("hr:city=islet").is_err());
        // …but they are fine as a source, which is how a country thins out its islets.
        assert!(PlaceTypeOverrides::from_str("no:islet=-").is_ok());
        assert!(PlaceTypeOverrides::from_str("no:islet=z14/xs").is_ok());
        // An islet has no default style to fall back on.
        assert!(PlaceTypeOverrides::from_str("no:islet=z14").is_err());

        assert!(PlaceTypeOverrides::from_str("hr:village=z13/huge").is_err());
        assert!(PlaceTypeOverrides::from_str("hr:village=zz").is_err());
        // Past the last zoom with place labels the rule would be a silent hide.
        assert!(PlaceTypeOverrides::from_str("hr:village=z18/xs").is_err());
    }
}

/// Static, server-side render configuration that does not vary per request.
#[derive(Clone, Debug)]
pub struct RenderConfig {
    pub svg_base_path: Arc<Path>,
    pub hillshading_base_path: Option<PathBuf>,
    pub hillshading_hierarchy: Option<HillshadingHierarchy>,
    pub contour_countries: Option<ContourCountries>,
    pub feature_line_mask_countries: Option<FeatureLineMaskCountries>,
    pub hillshading_max_open_per_country: usize,
    pub place_type_overrides: Option<Arc<PlaceTypeOverrides>>,
}
