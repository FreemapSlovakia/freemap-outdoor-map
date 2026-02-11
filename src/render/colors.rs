use cairo::Context;

pub type Color = (f64, f64, f64);

const M: i64 = 1_000_000;

pub const fn hsl_to_rgb(h: u16, s: u8, l: u8) -> Color {
    let h = h as i64 * M / 360; // Convert to range [0, 1]
    let s = s as i64 * M / 100; // Convert to range [0, 1]
    let l = l as i64 * M / 100; // Convert to range [0, 1]

    let (r, g, b) = if s == 0 {
        (l, l, l) // Achromatic
    } else {
        let q = if l < M / 2 {
            l * (M + s) / M
        } else {
            l + s - l * s / M
        };

        let p = 2 * l - q;
        (
            hue_to_rgb(p, q, h + M / 3),
            hue_to_rgb(p, q, h),
            hue_to_rgb(p, q, h - M / 3),
        )
    };

    const INV_255: f64 = 1.0 / 255.0;
    (
        (r * 255 / M) as f64 * INV_255,
        (g * 255 / M) as f64 * INV_255,
        (b * 255 / M) as f64 * INV_255,
    )
}

const fn hue_to_rgb(p: i64, q: i64, mut t: i64) -> i64 {
    if t < 0 {
        t += M;
    }

    if t > M {
        t -= M;
    }

    if t < M / 6 {
        p + (q - p) * 6 * t / M
    } else if t < M / 2 {
        q
    } else if t < M * 2 / 3 {
        p + (q - p) * (M * 2 / 3 - t) * 6 / M
    } else {
        p
    }
}

const fn parse_color(color: &str) -> Color {
    let bytes = color.as_bytes();

    if bytes.is_empty() {
        panic!("empty color");
    }
    const INV_255: f64 = 1.0 / 255.0;

    #[inline]
    const fn is_digit(b: u8) -> bool {
        b'0' <= b && b <= b'9'
    }

    #[inline]
    const fn skip_spaces(bytes: &[u8], mut i: usize) -> usize {
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        i
    }

    #[inline]
    const fn expect(bytes: &[u8], i: usize, c: u8) -> usize {
        if i >= bytes.len() || bytes[i] != c {
            panic!("invalid color");
        }
        i + 1
    }

    #[inline]
    const fn parse_uint(bytes: &[u8], mut i: usize) -> (i64, usize) {
        if i >= bytes.len() || !is_digit(bytes[i]) {
            panic!("invalid number");
        }

        let mut v: i64 = 0;

        while i < bytes.len() && is_digit(bytes[i]) {
            v = v * 10 + (bytes[i] - b'0') as i64;
            i += 1;
        }

        (v, i)
    }

    #[inline]
    const fn hex(c: u8) -> i64 {
        match c {
            b'0'..=b'9' => (c - b'0') as i64,
            b'a'..=b'f' => (10 + c - b'a') as i64,
            b'A'..=b'F' => (10 + c - b'A') as i64,
            _ => panic!("invalid hex"),
        }
    }

    if bytes[0] == b'#' {
        if bytes.len() != 7 {
            panic!("invalid hex length");
        }
        let r = (hex(bytes[1]) << 4) | hex(bytes[2]);
        let g = (hex(bytes[3]) << 4) | hex(bytes[4]);
        let b = (hex(bytes[5]) << 4) | hex(bytes[6]);
        return (r as f64 * INV_255, g as f64 * INV_255, b as f64 * INV_255);
    }

    if bytes.len() >= 4
        && bytes[0] == b'h'
        && bytes[1] == b's'
        && bytes[2] == b'l'
        && bytes[3] == b'('
    {
        let mut i = 4;
        let (h, j) = parse_uint(bytes, i);
        i = skip_spaces(bytes, j);
        i = expect(bytes, i, b',');
        i = skip_spaces(bytes, i);
        let (s, j) = parse_uint(bytes, i);
        i = expect(bytes, j, b'%');
        i = skip_spaces(bytes, i);
        i = expect(bytes, i, b',');
        i = skip_spaces(bytes, i);
        let (l, j) = parse_uint(bytes, i);
        i = expect(bytes, j, b'%');
        i = skip_spaces(bytes, i);
        i = expect(bytes, i, b')');
        if i != bytes.len() {
            panic!("trailing characters");
        }
        return hsl_to_rgb(h as u16, s as u8, l as u8);
    }

    if bytes.len() >= 4
        && bytes[0] == b'r'
        && bytes[1] == b'g'
        && bytes[2] == b'b'
        && bytes[3] == b'('
    {
        let mut i = 4;
        let (r, j) = parse_uint(bytes, i);
        i = skip_spaces(bytes, j);
        i = expect(bytes, i, b',');
        i = skip_spaces(bytes, i);
        let (g, j) = parse_uint(bytes, i);
        i = skip_spaces(bytes, j);
        i = expect(bytes, i, b',');
        i = skip_spaces(bytes, i);
        let (b, j) = parse_uint(bytes, i);
        i = skip_spaces(bytes, j);
        i = expect(bytes, i, b')');
        if i != bytes.len() {
            panic!("trailing characters");
        }
        return (r as f64 * INV_255, g as f64 * INV_255, b as f64 * INV_255);
    }

    panic!("unknown color format")
}

pub const ADMIN_BORDER: Color = parse_color("hsl(278, 100%, 50%)");
pub const AEROWAY: Color = parse_color("hsl(260, 10%, 50%)");
pub const ALLOTMENTS: Color = parse_color("hsl(50, 45%, 88%)");
pub const AREA_LABEL: Color = parse_color("hsl(0, 0%, 33%)");
pub const BEACH: Color = parse_color("hsl(60, 90%, 85%)");
pub const BROWNFIELD: Color = parse_color("hsl(30, 30%, 68%)");
pub const BUILDING: Color = parse_color("hsl(0, 0%, 50%)");
pub const BRIDLEWAY: Color = parse_color("hsl(120, 50%, 30%)");
pub const BRIDLEWAY2: Color = parse_color("hsl(120, 50%, 80%)");
pub const COLLEGE: Color = parse_color("hsl(60, 85%, 92%)");
pub const COMMERCIAL: Color = parse_color("hsl(320, 40%, 90%)");
pub const CONTOUR: Color = parse_color("hsl(0, 0%, 0%)");
pub const CYCLEWAY: Color = parse_color("hsl(282, 100%, 50%)");
pub const DAM: Color = parse_color("hsl(0, 0%, 70%)");
pub const FARMLAND: Color = parse_color("hsl(60, 70%, 95%)");
pub const FARMYARD: Color = parse_color("hsl(50, 44%, 85%)");
pub const FOREST: Color = parse_color("hsl(110, 60%, 83%)");
pub const GLOW: Color = parse_color("hsl(0, 33%, 70%)");
pub const GRASSY: Color = parse_color("hsl(100, 100%, 93%)");
pub const RECREATION_GROUND: Color = parse_color("hsl(90, 100%, 95%)");
pub const HEATH: Color = parse_color("hsl(85, 60%, 85%)");
pub const HOSPITAL: Color = parse_color("hsl(50, 85%, 92%)");
pub const INDUSTRIAL: Color = parse_color("hsl(0, 0%, 85%)");
pub const LANDFILL: Color = parse_color("hsl(0, 30%, 75%)");
pub const MILITARY: Color = parse_color("hsl(0, 96%, 39%)");
pub const NONE: Color = parse_color("hsl(0, 100%, 100%)");
pub const ORCHARD: Color = parse_color("hsl(90, 75%, 85%)");
pub const PARKING_STROKE: Color = parse_color("hsl(0, 30%, 75%)");
pub const PARKING: Color = parse_color("hsl(0, 20%, 88%)");
pub const PIER: Color = parse_color("hsl(0, 0%, 0%)");
pub const PIPELINE: Color = parse_color("hsl(0, 0%, 50%)");
pub const PISTE: Color = parse_color("hsl(0, 100%, 100%)");
pub const PISTE2: Color = parse_color("hsl(0, 0%, 62%)");
pub const PITCH_STROKE: Color = parse_color("hsl(110, 35%, 50%)");
pub const PITCH: Color = parse_color("hsl(110, 35%, 75%)");
pub const POWER_LINE: Color = parse_color("hsl(0, 0%, 0%)");
pub const POWER_LINE_MINOR: Color = parse_color("hsl(0, 0%, 50%)");
pub const PROTECTED: Color = parse_color("hsl(120, 75%, 25%)");
pub const SPECIAL_PARK: Color = parse_color("hsl(330, 75%, 25%)");
pub const GLACIER: Color = parse_color("hsl(216, 65%, 90%)");
pub const QUARRY: Color = parse_color("hsl(0, 0%, 78%)");
pub const RESIDENTIAL: Color = parse_color("hsl(100, 0%, 91%)");
pub const ROAD: Color = parse_color("hsl(40, 60%, 50%)");
pub const SCREE: Color = parse_color("hsl(0, 0%, 90%)");
pub const SCRUB: Color = parse_color("hsl(100, 70%, 86%)");
pub const SILO_STROKE: Color = parse_color("hsl(50, 20%, 30%)");
pub const SILO: Color = parse_color("hsl(50, 20%, 50%)");
pub const SUPERROAD: Color = parse_color("hsl(10, 60%, 60%)");
pub const TRACK: Color = parse_color("hsl(0, 33%, 25%)");
pub const WATER_LABEL_HALO: Color = parse_color("hsl(216, 30%, 100%)");
pub const WATER_LABEL: Color = parse_color("hsl(216, 100%, 50%)");
pub const WATER_SLIDE: Color = parse_color("hsl(180, 50%, 50%)");
pub const WATER: Color = parse_color("hsl(216, 65%, 70%)");
pub const RAIL_GLOW: Color = parse_color("hsl(0, 100%, 100%)");
pub const TRAM: Color = parse_color("hsl(0, 0%, 20%)");
pub const RAILWAY_DISUSED: Color = parse_color("hsl(0, 0%, 30%)");
pub const RAIL: Color = parse_color("hsl(0, 0%, 0%)");
pub const CONSTRUCTION_ROAD_1: Color = parse_color("hsl(60, 100%, 50%)");
pub const CONSTRUCTION_ROAD_2: Color = parse_color("hsl(0, 0%, 40%)");
pub const LOCALITY_LABEL: Color = parse_color("hsl(0, 0%, 40%)");
pub const BARRIERWAY: Color = parse_color("hsl(0, 100%, 50%)");
pub const BLACK: Color = parse_color("hsl(0, 0%, 0%)");
pub const WHITE: Color = parse_color("hsl(0, 100%, 100%)");
pub const SOLAR_BG: Color = parse_color("hsl(250, 63%, 60%)");
pub const SOLAR_FG: Color = parse_color("hsl(250, 57%, 76%)");
pub const TREE: Color = parse_color("hsl(120, 100%, 31%)");
pub const DAM_LINE: Color = parse_color("hsl(0, 0%, 40%)");
pub const SOLAR_PLANT_BORDER: Color = parse_color("hsl(250, 60%, 50%)");

pub trait ContextExt {
    fn set_source_color(&self, color: Color);

    fn set_source_color_a(&self, color: Color, alpha: f64);
}

impl ContextExt for Context {
    fn set_source_color(&self, color: Color) {
        self.set_source_rgb(color.0, color.1, color.2);
    }

    fn set_source_color_a(&self, color: Color, alpha: f64) {
        self.set_source_rgba(color.0, color.1, color.2, alpha);
    }
}

pub fn parse_hex_rgb(color: &str) -> Option<Color> {
    let bytes = color.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return None;
    }

    #[inline]
    fn hex(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(10 + c - b'a'),
            b'A'..=b'F' => Some(10 + c - b'A'),
            _ => None,
        }
    }

    let (Some(rh), Some(rl), Some(gh), Some(gl), Some(bh), Some(bl)) = (
        hex(bytes[1]),
        hex(bytes[2]),
        hex(bytes[3]),
        hex(bytes[4]),
        hex(bytes[5]),
        hex(bytes[6]),
    ) else {
        return None;
    };

    const INV_255: f64 = 1.0 / 255.0;

    Some((
        f64::from((rh << 4) | rl) * INV_255,
        f64::from((gh << 4) | gl) * INV_255,
        f64::from((bh << 4) | bl) * INV_255,
    ))
}
