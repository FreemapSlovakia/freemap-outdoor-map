use std::collections::BTreeSet;

/// The dataset codes that contributed pixels to one render.
///
/// Codes are namespaced — `osm`, `shading:<key>`, `contours:<key>` — because a
/// region's shading and its contours can come from different sources under
/// different licences. `<key>` is the hillshading-hierarchy / contour-country key
/// (`sk`, `de_by`, …), or `_` for a global fallback source. Titles and URLs are
/// not here; a client resolves the codes through `GET /licenses`.
///
/// This is the form the API speaks. [`encode`](Self::encode) is the shorter one
/// the tile cache and the headers carry — see there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribution(BTreeSet<String>);

/// Response header carrying [`Attribution::encode`], on tiles and on a finished
/// export's poll alike, so a client needs one parser. Present and empty means
/// nothing to credit; absent means the codes are unknown. Resolve them through
/// `GET /licenses`.
pub const ATTRIBUTION_HEADER: &str = "X-Attribution";

/// Code for the OpenStreetMap vector data every map render draws from.
pub const OSM: &str = "osm";

/// Key standing for a global fallback dataset, matching the `_` of
/// `--contour-countries` and the `_` hillshading dataset directory.
pub const FALLBACK_KEY: &str = "_";

const SHADING: &str = "shading:";
const CONTOURS: &str = "contours:";

impl Attribution {
    pub fn add_osm(&mut self) {
        self.0.insert(OSM.to_owned());
    }

    pub fn add_shading(&mut self, key: &str) {
        self.0.insert(format!("{SHADING}{key}"));
    }

    pub fn add_contours(&mut self, key: &str) {
        self.0.insert(format!("{CONTOURS}{key}"));
    }

    /// The long codes, in the namespace order the set sorts them by. What reads
    /// them for display orders them itself — see [`FALLBACK_KEY`].
    pub fn codes(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// The form every cached tile stores and `X-Attribution` sends: the codes
    /// comma-separated, each with its namespace shortened to one character — `o`,
    /// `s<key>`, `c<key>`. A tile near a triple border lists nine sources in 31
    /// bytes this way. [`encode_spaced`](Self::encode_spaced) is the same list for
    /// a header that cannot hold commas.
    ///
    /// Sorted by the long code, so the namespaces group and a given set of sources
    /// always encodes to the same bytes.
    ///
    /// The keys go in verbatim, which is what makes the form stable: they are the
    /// configuration's own identifiers, with no second numbering to keep in sync.
    /// They cannot contain the separator — `--hillshading-hierarchy` and
    /// `--contour-countries` split on `,` themselves.
    pub fn encode(&self) -> String {
        self.join(",")
    }

    /// The same short codes space-separated, for a `Server-Timing` `desc`. The
    /// `Server-Timing` grammar splits metrics on commas, and quoting one is not
    /// worth relying on across browsers — but it is the same spelling, so a client
    /// that reads either reads both.
    pub fn encode_spaced(&self) -> String {
        self.join(" ")
    }

    fn join(&self, separator: &str) -> String {
        self.0
            .iter()
            .map(|code| shorten(code))
            .collect::<Vec<_>>()
            .join(separator)
    }

    pub fn decode(payload: &str) -> Self {
        Self(
            payload
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(lengthen)
                .collect(),
        )
    }
}

fn shorten(code: &str) -> String {
    if code == OSM {
        return "o".to_owned();
    }

    if let Some(key) = code.strip_prefix(SHADING) {
        return format!("s{key}");
    }

    if let Some(key) = code.strip_prefix(CONTOURS) {
        return format!("c{key}");
    }

    code.to_owned()
}

fn lengthen(token: &str) -> String {
    let mut chars = token.chars();

    match (chars.next(), chars.as_str()) {
        (Some('o'), "") => OSM.to_owned(),
        (Some('s'), key) if !key.is_empty() => format!("{SHADING}{key}"),
        (Some('c'), key) if !key.is_empty() => format!("{CONTOURS}{key}"),
        // A namespace this build does not know, written by another one. Kept as it
        // stands so it surfaces as an unresolvable code rather than vanishing.
        _ => token.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::Attribution;

    #[test]
    fn the_short_form_groups_by_namespace() {
        let mut attribution = Attribution::default();
        attribution.add_osm();
        attribution.add_shading("de_by");
        attribution.add_contours("at");

        // Sorted by the long code, so the namespaces group and a given set of
        // sources always encodes to the same bytes.
        assert_eq!(attribution.encode(), "cat,o,sde_by");
        assert_eq!(attribution.encode_spaced(), "cat o sde_by");
        assert_eq!(Attribution::decode(&attribution.encode()), attribution);
    }

    #[test]
    fn an_unknown_namespace_survives_decoding() {
        // Neither dropped nor mangled: it comes back out as itself, so it shows up
        // as a code `/licenses` cannot resolve instead of silently disappearing.
        let decoded = Attribution::decode("o,x9,s,");

        // `o` expands, `s` and `x9` name no known namespace and stay as they are —
        // and all three come back out, so the round trip loses nothing.
        assert_eq!(decoded.encode(), "o,s,x9");
        assert_eq!(Attribution::decode(&decoded.encode()), decoded);
    }
}
