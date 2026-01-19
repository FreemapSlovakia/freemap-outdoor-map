use cairo::Context;

pub type Color = (i64, i64, i64);

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

    // Convert to range [0, 255]
    (r * 255 / M, g * 255 / M, b * 255 / M)
}

const fn hue_to_rgb(p: i64, q: i64, mut t: i64) -> i64 {
    if t < 0 {
        t += M;
    }
    if t > M {
        t -= M;
    }
    if t < M / 6 {
        return p + (q - p) * 6 * t / M;
    }
    if t < M / 2 {
        return q;
    }
    if t < M * 2 / 3 {
        return p + (q - p) * (M * 2 / 3 - t) * 6 / M;
    }
    p
}

pub const ADMIN_BORDER: Color = hsl_to_rgb(278, 100, 50);
pub const AEROWAY: Color = hsl_to_rgb(260, 10, 50);
pub const ALLOTMENTS: Color = hsl_to_rgb(50, 45, 88);
pub const AREA_LABEL: Color = hsl_to_rgb(0, 0, 33);
pub const BEACH: Color = hsl_to_rgb(60, 90, 85);
pub const BROWNFIELD: Color = hsl_to_rgb(30, 30, 68);
pub const BUILDING: Color = hsl_to_rgb(0, 0, 50);
pub const BRIDLEWAY: Color = hsl_to_rgb(120, 50, 30);
pub const BRIDLEWAY2: Color = hsl_to_rgb(120, 50, 80);
pub const COLLEGE: Color = hsl_to_rgb(60, 85, 92);
pub const COMMERCIAL: Color = hsl_to_rgb(320, 40, 90);
pub const CONTOUR: Color = hsl_to_rgb(0, 0, 0);
pub const CYCLEWAY: Color = hsl_to_rgb(282, 100, 50);
pub const DAM: Color = hsl_to_rgb(0, 0, 70);
pub const FARMLAND: Color = hsl_to_rgb(60, 70, 95);
pub const FARMYARD: Color = hsl_to_rgb(50, 44, 85);
pub const FOREST: Color = hsl_to_rgb(110, 60, 83);
pub const GLOW: Color = hsl_to_rgb(0, 33, 70);
pub const GRASSY: Color = hsl_to_rgb(100, 100, 93);
pub const RECREATION_GROUND: Color = hsl_to_rgb(90, 100, 95);
pub const HEATH: Color = hsl_to_rgb(85, 60, 85);
pub const HOSPITAL: Color = hsl_to_rgb(50, 85, 92);
pub const INDUSTRIAL: Color = hsl_to_rgb(0, 0, 85);
pub const LANDFILL: Color = hsl_to_rgb(0, 30, 75);
pub const MILITARY: Color = hsl_to_rgb(0, 96, 39);
pub const NONE: Color = hsl_to_rgb(0, 100, 100);
pub const ORCHARD: Color = hsl_to_rgb(90, 75, 85);
pub const PARKING_STROKE: Color = hsl_to_rgb(0, 30, 75);
pub const PARKING: Color = hsl_to_rgb(0, 20, 88);
pub const PIER: Color = hsl_to_rgb(0, 0, 0);
pub const PIPELINE: Color = hsl_to_rgb(0, 0, 50);
pub const PISTE: Color = hsl_to_rgb(0, 255, 255);
pub const PISTE2: Color = hsl_to_rgb(0, 0, 62);
pub const PITCH_STROKE: Color = hsl_to_rgb(110, 35, 50);
pub const PITCH: Color = hsl_to_rgb(110, 35, 75);
pub const POWER_LINE: Color = hsl_to_rgb(0, 0, 0);
pub const POWER_LINE_MINOR: Color = hsl_to_rgb(0, 0, 50);
pub const PROTECTED: Color = hsl_to_rgb(120, 75, 25);
pub const SPECIAL_PARK: Color = hsl_to_rgb(330, 75, 25);
pub const GLACIER: Color = hsl_to_rgb(216, 65, 90);
pub const QUARRY: Color = hsl_to_rgb(0, 0, 78);
pub const RESIDENTIAL: Color = hsl_to_rgb(100, 0, 91);
pub const ROAD: Color = hsl_to_rgb(40, 60, 50);
pub const SCREE: Color = hsl_to_rgb(0, 0, 90);
pub const SCRUB: Color = hsl_to_rgb(100, 70, 86);
pub const SILO_STROKE: Color = hsl_to_rgb(50, 20, 30);
pub const SILO: Color = hsl_to_rgb(50, 20, 50);
pub const SUPERROAD: Color = hsl_to_rgb(10, 60, 60);
pub const TRACK: Color = hsl_to_rgb(0, 33, 25);
pub const WATER_LABEL_HALO: Color = hsl_to_rgb(216, 30, 100);
pub const WATER_LABEL: Color = hsl_to_rgb(216, 100, 50);
pub const WATER_SLIDE: Color = hsl_to_rgb(180, 50, 50);
pub const WATER: Color = hsl_to_rgb(216, 65, 70);
pub const RAIL_GLOW: Color = hsl_to_rgb(0, 100, 100);
pub const TRAM: Color = hsl_to_rgb(0, 0, 20);
pub const RAILWAY_DISUSED: Color = hsl_to_rgb(0, 0, 30);
pub const RAIL: Color = hsl_to_rgb(0, 0, 0);
pub const CONSTRUCTION_ROAD_1: Color = hsl_to_rgb(60, 100, 50);
pub const CONSTRUCTION_ROAD_2: Color = hsl_to_rgb(0, 0, 40);
pub const LOCALITY_LABEL: Color = hsl_to_rgb(0, 0, 40);
pub const BARRIERWAY: Color = hsl_to_rgb(0, 100, 50);
pub const BLACK: Color = hsl_to_rgb(0, 0, 0);
pub const WHITE: Color = hsl_to_rgb(0, 100, 100);
pub const SOLAR_BG: Color = hsl_to_rgb(250, 63, 60);
pub const SOLAR_FG: Color = hsl_to_rgb(250, 57, 76);
pub const TREE: Color = hsl_to_rgb(120, 100, 31);
pub const DAM_LINE: Color = hsl_to_rgb(0, 0, 40);
pub const SOLAR_PLANT_BORDER: Color = hsl_to_rgb(250, 60, 50);

pub trait ContextExt {
    fn set_source_color(&self, color: Color);

    fn set_source_color_a(&self, color: Color, alpha: f64);
}

impl ContextExt for Context {
    fn set_source_color(&self, color: Color) {
        self.set_source_rgb(
            color.0 as f64 / 255.0,
            color.1 as f64 / 255.0,
            color.2 as f64 / 255.0,
        );
    }

    fn set_source_color_a(&self, color: Color, alpha: f64) {
        self.set_source_rgba(
            color.0 as f64 / 255.0,
            color.1 as f64 / 255.0,
            color.2 as f64 / 255.0,
            alpha,
        );
    }
}

pub fn parse_hex_rgb(color: &str) -> Option<(f64, f64, f64)> {
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
