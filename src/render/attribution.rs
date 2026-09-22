use std::collections::BTreeSet;

/// The dataset codes that contributed pixels to one render.
///
/// Codes are namespaced — `osm`, `shading:<key>`, `contours:<key>` — because a
/// region's shading and its contours can come from different sources under
/// different licences. `<key>` is the hillshading-hierarchy / contour-country key
/// (`sk`, `de_by`, …), or `_` for a global fallback source. Titles and URLs are
/// not here; a client resolves the codes through `GET /licenses`.
///
/// This is the form the API speaks. [`encode`](Self::encode) writes a shorter one
/// into the image — see there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribution(BTreeSet<String>);

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

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The form stored in the image, which every cached tile carries: the codes
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

/// Longest payload a single JPEG segment can hold: the 16-bit length field counts
/// itself.
const MAX_COM_PAYLOAD: usize = u16::MAX as usize - 2;

/// Writes the codes into the JPEG as a `COM` segment placed first, right after
/// `SOI`, so that a reader gets them from a fixed offset with no JPEG parser:
/// `FF D8 | FF FE | len_hi len_lo | payload`.
///
/// A payload too long for one segment is dropped rather than truncated: a
/// half-code list would credit the wrong sources, where none at all just leaves
/// the tile without an `attr` metric.
pub fn insert_jpeg_com(jpeg: &mut Vec<u8>, payload: &str) {
    let payload = payload.as_bytes();

    if payload.len() > MAX_COM_PAYLOAD || jpeg.len() < 2 || jpeg[0..2] != [0xFF, 0xD8] {
        return;
    }

    let mut segment = Vec::with_capacity(payload.len() + 4);
    segment.extend_from_slice(&[0xFF, 0xFE]);
    segment.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    segment.extend_from_slice(payload);

    jpeg.splice(2..2, segment);
}

/// Bytes of a JPEG's head that must be readable for [`parse_jpeg_com`] to see the
/// whole segment. Serving a cached tile reads this much and no more.
pub const JPEG_COM_HEAD_LEN: usize = 1024;

/// The payload of a `COM` segment written by [`insert_jpeg_com`], or `None` when
/// the JPEG does not start with one — which is how a tile cached before tiles
/// carried attribution is served without codes instead of breaking.
pub fn parse_jpeg_com(head: &[u8]) -> Option<&str> {
    if head.len() < 6 || head[0..2] != [0xFF, 0xD8] || head[2..4] != [0xFF, 0xFE] {
        return None;
    }

    let length = u16::from_be_bytes([head[4], head[5]]) as usize;
    let end = 4usize.checked_add(length)?;

    if length < 2 || end > head.len() {
        return None;
    }

    std::str::from_utf8(&head[6..end]).ok()
}

/// `tEXt` keyword for the code list. Not one of PNG's registered keywords, so it
/// cannot be mistaken for human-readable copyright text.
const PNG_KEYWORD: &[u8] = b"map-attribution";

/// Writes the codes into the PNG as a `tEXt` chunk placed right after `IHDR`,
/// which is a fixed offset: 8-byte signature + a 25-byte `IHDR` chunk.
pub fn insert_png_text(png: &mut Vec<u8>, payload: &str) {
    const IHDR_END: usize = 33;

    if png.len() < IHDR_END || &png[12..16] != b"IHDR" {
        return;
    }

    let mut data = Vec::with_capacity(PNG_KEYWORD.len() + payload.len() + 1);
    data.extend_from_slice(PNG_KEYWORD);
    data.push(0);
    data.extend_from_slice(payload.as_bytes());

    let mut chunk = Vec::with_capacity(data.len() + 12);
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&data);

    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());

    png.splice(IHDR_END..IHDR_END, chunk);
}

/// PNG's CRC-32 (IEEE, reflected). Bitwise rather than table-driven: it runs once
/// per exported image over a few dozen bytes.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;

    for &byte in bytes {
        crc ^= u32::from(byte);

        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xEDB8_8320
            };
        }
    }

    !crc
}

#[cfg(test)]
mod tests {
    use super::{Attribution, crc32, insert_jpeg_com, insert_png_text, parse_jpeg_com};

    #[test]
    fn a_com_segment_round_trips_through_the_head_bytes() {
        let mut attribution = Attribution::default();
        attribution.add_osm();
        attribution.add_shading("de_by");
        attribution.add_contours("at");

        // Sorted by the long code, so the namespaces group and the payload for a
        // given set of sources is always the same bytes.
        let payload = attribution.encode();
        assert_eq!(payload, "cat,o,sde_by");
        assert_eq!(attribution.encode_spaced(), "cat o sde_by");

        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        insert_jpeg_com(&mut jpeg, &payload);

        assert_eq!(&jpeg[0..4], &[0xFF, 0xD8, 0xFF, 0xFE]);
        assert_eq!(parse_jpeg_com(&jpeg), Some(payload.as_str()));
        assert_eq!(
            Attribution::decode(parse_jpeg_com(&jpeg).expect("segment present")),
            attribution
        );

        // The original JPEG follows the segment untouched.
        assert_eq!(&jpeg[jpeg.len() - 4..], &[0xFF, 0xE0, 0x00, 0x10]);
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

    #[test]
    fn a_jpeg_without_the_segment_reads_as_unknown() {
        assert_eq!(parse_jpeg_com(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]), None);
        assert_eq!(parse_jpeg_com(&[0xFF, 0xD8]), None);
        assert_eq!(parse_jpeg_com(&[]), None);
        // Truncated head: the segment claims more bytes than were read.
        assert_eq!(parse_jpeg_com(&[0xFF, 0xD8, 0xFF, 0xFE, 0xFF, 0x00]), None);
    }

    #[test]
    fn an_oversized_payload_is_dropped_rather_than_truncated() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        insert_jpeg_com(&mut jpeg, &"x".repeat(u16::MAX as usize));

        assert_eq!(jpeg, vec![0xFF, 0xD8, 0xFF, 0xE0]);
    }

    /// The chunk/segment offsets are hard-coded, so check them against what the
    /// encoders this crate actually uses produce, and that both stay decodable.
    #[test]
    fn real_encoder_output_survives_the_insertion() {
        use cairo::{Format, ImageSurface};
        use image::{ExtendedColorType, ImageDecoder, ImageEncoder, codecs};

        let payload = "osm,shading:sk";

        let surface = ImageSurface::create(Format::Rgb24, 8, 8).expect("surface");
        let mut png = Vec::new();
        surface.write_to_png(&mut png).expect("png written");
        insert_png_text(&mut png, payload);

        let decoder =
            codecs::png::PngDecoder::new(std::io::Cursor::new(&png)).expect("png decodes");
        assert_eq!(decoder.dimensions(), (8, 8));

        let mut jpeg = Vec::new();
        codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .write_image(&[0u8; 8 * 8 * 3], 8, 8, ExtendedColorType::Rgb8)
            .expect("jpeg written");
        insert_jpeg_com(&mut jpeg, payload);

        assert_eq!(parse_jpeg_com(&jpeg), Some(payload));

        let decoder =
            codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&jpeg)).expect("jpeg decodes");
        assert_eq!(decoder.dimensions(), (8, 8));
    }

    #[test]
    fn the_png_chunk_lands_after_ihdr_with_a_valid_crc() {
        let mut png = Vec::new();
        png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 13]);
        png.extend_from_slice(&[0; 4]); // IHDR CRC
        png.extend_from_slice(b"rest");

        insert_png_text(&mut png, "osm");

        let length = u32::from_be_bytes([png[33], png[34], png[35], png[36]]) as usize;
        assert_eq!(&png[37..41], b"tEXt");
        assert_eq!(&png[41..41 + length], b"map-attribution\0osm");

        let crc_at = 37 + 4 + length;
        assert_eq!(
            u32::from_be_bytes([png[crc_at], png[crc_at + 1], png[crc_at + 2], png[crc_at + 3]]),
            crc32(&png[37..37 + 4 + length])
        );
        assert_eq!(&png[png.len() - 4..], b"rest");
    }
}
