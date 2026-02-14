use serde::Serialize;

#[derive(Copy, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    RoadsAndPaths,
    Railway,
    Landcover,
    Borders,
    Accommodation,
    NaturalPoi,
    GastroPoi,
    Water,
    Institution,
    Sport,
    Poi,
    Terrain,
    Other,
}
