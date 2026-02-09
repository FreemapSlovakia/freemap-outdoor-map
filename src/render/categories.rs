use serde::Serialize;

#[derive(Copy, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Communications,
    Railway,
    Landcover,
    Borders,
    Accomodation,
    NaturalPoi,
    GastroPoi,
    Water,
    Institution,
    Sport,
    Poi,
    Terrain,
    Other,
}
