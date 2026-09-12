use crate::render::colors;
use serde::Serialize;

/// The group a rendered feature is listed under in the map legend.
///
/// One variant is one section of the legend, so these are reader-facing groups - "where
/// would someone go looking for this?" - not renderer internals. Points, lines and areas
/// all share the set: a barrier is a [`Category::Barrier`] whether it is drawn as a gate
/// icon or as a fence line, and the layer that happens to draw it never decides the group.
///
/// The kebab-case name is the key the frontend looks up in its legend translations
/// (`freemap-v3-react`, `src/features/legend/translations/`), and the order of the keys
/// there is the order the sections appear in. A variant with no translation shows up in
/// the UI as its raw key, at the bottom - so a new variant needs an entry there too.
///
/// The split follows the sections of OpenStreetMap Carto's symbol list
/// (<https://wiki.openstreetmap.org/wiki/OpenStreetMap_Carto/Symbols>), bent towards an
/// outdoor map: waymarking is listed with the paths it marks rather than as street
/// furniture, and the things that decide whether a walk works at all - water, terrain,
/// barriers, shelter - get sections of their own instead of sharing a catch-all.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// Roads, tracks and paths, the routes signed along them, and the waymarking itself
    /// (guideposts, route markers) - which is part of the path to anyone using it, not a
    /// POI standing beside it.
    RoadsAndPaths,
    /// Railway lines and the stations and halts on them.
    Railway,
    /// Getting about by anything but rail: buses, taxis, ferries, aerialways, airfields,
    /// and what a car needs (parking, fuel, charging).
    Transport,
    /// Watercourses, water bodies and everything about water on the ground: springs,
    /// waterfalls, fords, wells and the works that move it.
    Water,
    /// The shape of the ground where it blocks or channels movement: cliffs, embankments,
    /// gullies, and obstacles across a path.
    Terrain,
    /// Named natural features - summits, saddles, caves, rocks, notable trees.
    NaturalPoi,
    /// Area fills for what covers the ground: forest, meadow, scree, built-up land.
    Landcover,
    /// Administrative and protected-area boundaries.
    Borders,
    /// Somewhere to spend the night or wait out the weather: hotels, huts, and the
    /// shelters a walker can stop in. Not a picnic shelter - that is a roof over a picnic
    /// table, and belongs with the rest of the picnic furniture in [`Category::Facility`].
    Accommodation,
    /// Places that serve food and drink.
    GastroPoi,
    /// Places that sell things.
    Shop,
    /// Sport, recreation and wellness, whether a pitch, a via ferrata or a sauna.
    Sport,
    /// What a visitor comes for or navigates by: viewpoints, attractions, observation
    /// towers, and the information boards and offices that explain them.
    Tourism,
    /// Culture and entertainment: museums, galleries, theatres, cinemas, artwork, nightlife.
    Culture,
    /// The past on the ground: castles, monuments, memorials, old mines, ruins.
    Historic,
    /// Places of worship and wayside religious objects.
    Religion,
    /// Public and civic services: town halls, emergency services, schools, post.
    Institution,
    /// Where money is handled: banks, ATMs, exchange offices.
    Finance,
    /// Healthcare, human and animal.
    Health,
    /// Built structures that are landmarks rather than destinations: towers, masts,
    /// chimneys, windmills, working mines, power lines and pipelines.
    ManMade,
    /// Things that stand in the way and decide whether you get through: gates, bollards,
    /// stiles, cattle grids, fences and walls.
    Barrier,
    /// Small facilities you use in passing: toilets, benches, picnic sites and the
    /// shelters over them, waste bins, post boxes.
    Facility,
    /// Everything that fits none of the above: the odd countryside object (bee hives,
    /// hunting stands, game feeding places), buildings, rendering hints, and the entries
    /// that demonstrate a modifier rather than a feature.
    Other,
}

impl Category {
    /// Whether a POI you may not get into is worth dimming.
    ///
    /// The restriction always shows as a glow; this is about whether it also empties the
    /// POI of value. A private playground or car park is dead weight, but a landmark is
    /// exactly as useful for navigating by - and a private gate more so, since the
    /// restriction is the whole reason it is drawn.
    pub const fn fades_when_restricted(self) -> bool {
        use Category::{
            Accommodation, Barrier, Borders, Culture, Facility, Finance, GastroPoi, Health,
            Historic, Institution, Landcover, ManMade, NaturalPoi, Other, Railway, Religion,
            RoadsAndPaths, Shop, Sport, Terrain, Tourism, Transport, Water,
        };

        match self {
            // Accommodation counts as a landmark: `access=private` on lodging usually
            // means the grounds, not that you cannot book, and a named chalet is one.
            NaturalPoi | Terrain | Barrier | ManMade | Water | Historic | Religion
            | RoadsAndPaths | Accommodation | Landcover | Borders => false,
            Shop | GastroPoi | Sport | Health | Facility | Culture | Institution | Finance
            | Transport | Tourism | Railway | Other => true,
        }
    }

    /// Default colour for this category's POI icons; `Extra::color` overrides it per type.
    ///
    /// Roughly 20 `Shop` entries, plus `police`/`fire_station`/`atm`, override to black
    /// because a walker actually uses them - so those legend headings do list icons in
    /// two colours. If that split hardens, it wants to be a category, not 23 overrides.
    pub const fn icon_color(self) -> colors::Color {
        use Category::{
            Accommodation, Barrier, Borders, Culture, Facility, Finance, GastroPoi, Health,
            Historic, Institution, Landcover, ManMade, NaturalPoi, Other, Railway, Religion,
            RoadsAndPaths, Shop, Sport, Terrain, Tourism, Transport, Water,
        };

        match self {
            Water => colors::POI_WATER,
            GastroPoi => colors::POI_GASTRO,
            Health => colors::POI_HEALTH,
            Sport => colors::POI_SPORT,
            Terrain => colors::POI_OBSTACLE,
            Accommodation => colors::POI_ACCOMMODATION,
            Shop | Institution | Finance => colors::POI_MUTED,
            // Black is the map's most prominent ink, so it is the default: what a reader
            // navigates by, plus the groups not yet judged on a real tile.
            RoadsAndPaths | Railway | Transport | NaturalPoi | Landcover | Borders | Tourism
            | Culture | Historic | Religion | ManMade | Barrier | Facility | Other => {
                colors::POI_BLACK
            }
        }
    }
}
