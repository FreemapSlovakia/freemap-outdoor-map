use geo::{Coord, Geometry, MapCoordsInPlace};
use geojson::GeoJson;
use proj::Proj;
use std::{cell::Cell, fs::File, io::BufReader, path::Path};

pub fn load_geometry_from_geojson(path: &Path) -> Result<Geometry, String> {
    let file = File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;

    let reader = BufReader::new(file);

    let geojson: GeoJson = serde_json::from_reader(reader)
        .map_err(|err| format!("parse {}: {err}", path.display()))?;

    let geometry: Geometry = Geometry::try_from(geojson)
        .map_err(|err| format!("convert {} to geo geometry: {err}", path.display()))?;

    project_to_web_mercator(geometry)
}

fn project_to_web_mercator(mut geometry: Geometry) -> Result<Geometry, String> {
    let proj = Proj::new_known_crs("EPSG:4326", "EPSG:3857", None)
        .map_err(|err| format!("failed to create 4326->3857 projection: {err}"))?;

    let failed = Cell::new(false);
    geometry.map_coords_in_place(|coord: Coord| match proj.convert((coord.x, coord.y)) {
        Ok((x, y)) => Coord { x, y },
        Err(_) => {
            failed.set(true);
            coord
        }
    });

    if failed.get() {
        Err("failed to project some mask coordinates to EPSG:3857".into())
    } else {
        Ok(geometry)
    }
}
