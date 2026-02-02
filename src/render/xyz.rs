use crate::render::size::Size;
use geo::Rect;

const EARTH_RADIUS: f64 = 6_378_137.0; // Equatorial radius of the Earth in meters (WGS 84)

const HALF_CIRCUMFERENCE: f64 = std::f64::consts::PI * EARTH_RADIUS;

pub fn bbox_size_in_pixels(bbox: Rect<f64>, zoom: f64) -> Size<u32> {
    let resolution = 2.0 * HALF_CIRCUMFERENCE / (256.0 * zoom.exp2());

    Size::new(
        (bbox.width() / resolution).round() as u32,
        (bbox.height() / resolution).round() as u32,
    )
}

pub fn to_absolute_pixel_coords(x: f64, y: f64, zoom: u8) -> (f64, f64) {
    // Tile size in pixels (usually 256 or 512)
    let tile_size: f64 = 256.0;

    // Earth's radius in the same units as x, y (meters for EPSG:3857)

    // Total number of tiles in one row or column at the given zoom level
    let num_tiles = 2f64.powi(zoom as i32);

    // Total map circumference at this zoom level
    let total_map_circumference = num_tiles * tile_size;

    // Convert x, y to pixel coordinates
    let pixel_x = (x + HALF_CIRCUMFERENCE) / (2.0 * HALF_CIRCUMFERENCE) * total_map_circumference;
    let pixel_y = (HALF_CIRCUMFERENCE - y) / (2.0 * HALF_CIRCUMFERENCE) * total_map_circumference;

    (pixel_x, pixel_y)
}

pub fn perpendicular_distance(point1: (f64, f64), point2: (f64, f64), theta: f64) -> f64 {
    let (x1, y1) = point1;
    let (x2, y2) = point2;

    // Convert angle to radians and calculate direction vector of the line
    let theta_radians = theta.to_radians();
    let d = (theta_radians.cos(), theta_radians.sin());

    // Vector from point1 to point2
    let v = (x2 - x1, y2 - y1);

    // Calculate the cross product magnitude (z-component of 3D cross product)
    // Cross product in 2D (extended to 3D): a_x * b_y - a_y * b_x
    v.0.mul_add(d.1, -(v.1 * d.0))
}
