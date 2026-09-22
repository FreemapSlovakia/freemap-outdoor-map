use crate::app::server::app_state::AppState;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Response, StatusCode, header},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

/// One source behind a dataset. A dataset can have several — Belgium's relief is
/// two regional models under one code.
///
/// Titles are the rights-holder's own attribution string and the licence's name
/// — the same in every language — so the document is not localized and one `ETag`
/// covers it for every client.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct License {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// OpenStreetMap is not a configurable dataset — every map render draws from it,
/// and its attribution is fixed.
fn osm_license() -> License {
    License {
        title: "© OpenStreetMap contributors".to_owned(),
        url: Some("https://osm.org/copyright".to_owned()),
    }
}

/// `attribution.json`, sitting next to a hillshading dataset's `final.tif`.
///
/// It lives with the data rather than in a central list because that is where the
/// licence is a fact: the script that downloads a DEM knows its terms, adding a
/// dataset is creating its directory, and deleting one takes its licence with it.
#[derive(Debug, Deserialize)]
struct DatasetAttribution {
    /// Which code namespaces these sources answer for.
    ///
    /// Contours are opt-in even though they are normally contoured from this very
    /// DEM, because a region that took its contours from somewhere else must not
    /// silently credit this dataset for them — mis-crediting is worse than the
    /// unresolved code that leaving it out produces.
    #[serde(default = "shading_only")]
    covers: Vec<String>,
    sources: Vec<License>,
}

fn shading_only() -> Vec<String> {
    vec![SHADING.to_owned()]
}

const SHADING: &str = "shading";
const CONTOURS: &str = "contours";

/// The `code -> sources` dictionary, pre-serialized: it is the same bytes for
/// every request and only changes when the operator adds a dataset.
pub struct LicenseCatalog {
    body: Bytes,
    etag: String,
}

impl LicenseCatalog {
    /// Collects every dataset's `attribution.json` under `hillshading_base`, adds
    /// the built-in OpenStreetMap entry, then overlays `overrides` — a JSON file
    /// of the same `code -> sources` shape, for anything that has no dataset
    /// directory or needs correcting without touching the data volume.
    ///
    /// Also returns what looked wrong, for the caller to log: a dataset that can
    /// be credited but says nothing about itself is the mistake this catches.
    pub fn build(
        hillshading_base: Option<&Path>,
        shading_keys: &[&str],
        contour_keys: &[&str],
        overrides: Option<&Path>,
    ) -> (Self, Vec<String>) {
        let mut licenses: BTreeMap<String, Vec<License>> = BTreeMap::new();
        let mut warnings = Vec::new();
        let mut undeclared: Vec<(&str, std::path::PathBuf)> = Vec::new();

        licenses.insert(crate::render::OSM_CODE.to_owned(), vec![osm_license()]);

        if let Some(base) = hillshading_base {
            for key in shading_keys {
                let path = base.join(key).join("attribution.json");

                let attribution = match std::fs::read_to_string(&path) {
                    Ok(text) => match serde_json::from_str::<DatasetAttribution>(&text) {
                        Ok(attribution) => attribution,
                        Err(err) => {
                            warnings.push(format!("parse {}: {err}", path.display()));
                            continue;
                        }
                    },
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        // Held back until the overrides have had their say: one of
                        // them may well be what this dataset has instead of a file.
                        undeclared.push((*key, path));
                        continue;
                    }
                    Err(err) => {
                        warnings.push(format!("open {}: {err}", path.display()));
                        continue;
                    }
                };

                for namespace in &attribution.covers {
                    match namespace.as_str() {
                        SHADING | CONTOURS => {
                            licenses.insert(
                                format!("{namespace}:{key}"),
                                attribution.sources.clone(),
                            );
                        }
                        other => warnings.push(format!(
                            "{}: unknown namespace '{other}' in `covers`",
                            path.display()
                        )),
                    }
                }
            }
        }

        if let Some(path) = overrides {
            match std::fs::read_to_string(path)
                .map_err(|err| format!("open {}: {err}", path.display()))
                .and_then(|text| {
                    serde_json::from_str::<BTreeMap<String, Vec<License>>>(&text)
                        .map_err(|err| format!("parse {}: {err}", path.display()))
                }) {
                Ok(overrides) => licenses.extend(overrides),
                Err(err) => warnings.push(err),
            }
        }

        for (key, path) in undeclared {
            if !licenses.contains_key(&format!("{SHADING}:{key}")) {
                warnings.push(format!(
                    "no {} — shading:{key} will not resolve",
                    path.display()
                ));
            }
        }

        // A contour source whose dataset did not opt into `contours` — usually the
        // `covers` line was forgotten rather than the contours really coming from
        // somewhere else.
        for key in contour_keys {
            let code = format!("{CONTOURS}:{key}");

            if !licenses.contains_key(&code) {
                warnings.push(format!(
                    "nothing resolves {code} — add \"{CONTOURS}\" to `covers` in the {key} dataset's attribution.json, or list the code in --licenses"
                ));
            }
        }

        (Self::new(&licenses), warnings)
    }

    fn new(licenses: &BTreeMap<String, Vec<License>>) -> Self {
        let body = serde_json::to_string(licenses).expect("serialize licenses");

        let mut hasher = DefaultHasher::new();
        body.hash(&mut hasher);

        Self {
            etag: format!("\"{:016x}\"", hasher.finish()),
            body: Bytes::from(body),
        }
    }
}

pub async fn get(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let catalog = &state.licenses;

    // Revalidate rather than hard-cache: a client holding a code it cannot resolve
    // is the failure this endpoint exists to prevent, and a 304 costs nothing.
    let builder = Response::builder()
        .header(header::ETAG, &catalog.etag)
        .header(header::CACHE_CONTROL, "no-cache");

    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|tag| tag.trim() == catalog.etag))
    {
        return builder
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .expect("not modified body");
    }

    builder
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        // A refcount bump, not a copy of the whole dictionary.
        .body(Body::from(catalog.body.clone()))
        .expect("licenses body")
}

#[cfg(test)]
mod tests {
    use super::LicenseCatalog;
    use std::{fs, path::PathBuf};

    fn dataset(base: &std::path::Path, key: &str, body: &str) {
        let dir = base.join(key);
        fs::create_dir_all(&dir).expect("dataset dir");
        fs::write(dir.join("attribution.json"), body).expect("attribution.json");
    }

    fn body(catalog: &LicenseCatalog) -> String {
        String::from_utf8(catalog.body.to_vec()).expect("utf-8 body")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("licenses-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn datasets_answer_for_the_namespaces_they_cover() {
        let base = scratch("covers");

        dataset(
            &base,
            "sk",
            r#"{"covers":["shading","contours"],"sources":[{"title":"DMR 5.0","url":"https://x"}]}"#,
        );
        // No `covers`, so shading only — its contours came from somewhere else.
        dataset(&base, "pl", r#"{"sources":[{"title":"NMT"}]}"#);

        let (catalog, warnings) =
            LicenseCatalog::build(Some(&base), &["sk", "pl", "at"], &["sk", "pl"], None);

        assert!(body(&catalog).contains(r#""shading:sk":[{"title":"DMR 5.0","url":"https://x"}]"#));
        assert!(body(&catalog).contains(r#""contours:sk""#));
        assert!(body(&catalog).contains(r#""shading:pl":[{"title":"NMT"}]"#));
        // Opting out of contours leaves the code unresolved rather than crediting
        // this dataset for lines it did not produce.
        assert!(!body(&catalog).contains(r#""contours:pl""#));
        // OSM is built in, not configured.
        assert!(body(&catalog).contains(r#""osm":[{"title":"© OpenStreetMap contributors""#));

        // The dataset that said nothing, and the contour code nothing resolves.
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("shading:at will not resolve")));
        assert!(warnings.iter().any(|w| w.contains("nothing resolves contours:pl")));
    }

    #[test]
    fn the_override_file_replaces_what_a_dataset_said() {
        let base = scratch("overrides");
        dataset(&base, "sk", r#"{"sources":[{"title":"stale"}]}"#);

        let overrides = base.join("licenses.json");
        fs::write(
            &overrides,
            r#"{"shading:sk":[{"title":"corrected"}],"contours:_":[{"title":"GEDTM30"}]}"#,
        )
        .expect("overrides");

        let (catalog, warnings) =
            LicenseCatalog::build(Some(&base), &["sk"], &["_"], Some(&overrides));

        assert!(body(&catalog).contains(r#""shading:sk":[{"title":"corrected"}]"#));
        assert!(!body(&catalog).contains("stale"));
        // A code with no dataset directory of its own is what overrides are for.
        assert!(body(&catalog).contains(r#""contours:_":[{"title":"GEDTM30"}]"#));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_override_silences_the_warning_for_a_dataset_with_no_file() {
        let base = scratch("rescued");

        let overrides = base.join("licenses.json");
        fs::write(&overrides, r#"{"shading:_":[{"title":"GEDTM30"}]}"#).expect("overrides");

        // No `_` dataset directory at all: the override is what answers for it, so
        // there is nothing to warn about.
        let (catalog, warnings) =
            LicenseCatalog::build(Some(&base), &["_"], &[], Some(&overrides));

        assert!(body(&catalog).contains(r#""shading:_""#));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_broken_file_is_reported_rather_than_fatal() {
        let base = scratch("broken");
        dataset(&base, "sk", "{ not json");

        let (catalog, warnings) = LicenseCatalog::build(Some(&base), &["sk"], &[], None);

        assert!(body(&catalog).contains(r#""osm""#));
        assert!(!body(&catalog).contains("shading:sk"));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("parse "));
    }
}
