use crate::render::{
    Feature,
    categories::Category,
    collision::Collision,
    colors::{self, Color},
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        text::{TextOptions, draw_text},
    },
    layer_render_error::{LayerRenderError, LayerRenderResult},
    projectable::TileProjectable,
    regex_replacer::{Replacement, build_replacements, replace},
    svg_repo::{self, Options, SvgRepo},
};
use cairo::Context;
use core::f64;
use cosmic_text::{Style, Weight};
use geo::{Point, Rect};
use std::fmt::Write as _;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

#[derive(Clone)]
struct Extra<'a> {
    replacements: Vec<Replacement<'a>>,
    icon: Option<&'a str>,
    font_size: f64,
    weight: Weight,
    text_color: Option<Color>,
    max_zoom: u8,
    /// Overrides the category colour, for the odd type that must break its category's
    /// rule - the red volcano, the village shop drawn black among the grey ones.
    color: Option<Color>,
    halo: bool,
    /// Label this POI with its bare elevation when it has no name. Only for spot
    /// heights, whose elevation *is* their label; every other `with_ele` type shows the
    /// elevation as a second line under a name, and labelling an unnamed one would put
    /// a naked number on the map where a reader expects a summit. Implies `with_ele`.
    ele_only_label: bool,
}

impl Default for Extra<'_> {
    fn default() -> Self {
        Self {
            replacements: vec![],
            icon: None,
            font_size: 12.0,
            weight: Weight::NORMAL,
            text_color: None,
            max_zoom: u8::MAX,
            color: None,
            halo: true,
            ele_only_label: false,
        }
    }
}

pub struct Def {
    min_zoom: u8,
    min_text_zoom: u8,
    with_ele: bool,
    natural: bool,
    pub category: Category,
    extra: Extra<'static>,
}

impl Def {
    pub(crate) const fn is_active_at(&self, zoom: u8) -> bool {
        self.min_zoom <= zoom && self.extra.max_zoom >= zoom
    }

    /// Zooms this definition is drawn at, both ends inclusive.
    pub(crate) const fn zoom_span(&self) -> (u8, u8) {
        (self.min_zoom, self.extra.max_zoom)
    }

    /// The definition's own colour if it has one, else its category's.
    pub(crate) fn color(&self) -> Color {
        color_of(self.category, &self.extra)
    }

    pub(crate) fn icon_key<'a>(&'a self, typ: &'a str) -> &'a str {
        icon_key_of(&self.extra, typ)
    }
}

fn color_of(category: Category, extra: &Extra) -> Color {
    extra.color.unwrap_or_else(|| category.icon_color())
}

fn icon_key_of<'a>(extra: &'a Extra<'a>, typ: &'a str) -> &'a str {
    extra.icon.unwrap_or(typ)
}

type PoiEntry = (u8, u8, bool, bool, Category, &'static str, Extra<'static>);

static POI_ENTRIES: LazyLock<Vec<PoiEntry>> = LazyLock::new(|| {
    const N: bool = false;
    const Y: bool = true;
    const NN: u8 = u8::MAX;

    let spring_replacements = build_replacements(&[
        (r"\b[Mm]inerálny\b", "min."),
        (r"\b[Pp]rameň\b", "prm."),
        (r"\b[Ss]tud(ničk|ň)a\b", "stud."),
        (r"\b[Vv]yvieračka\b", "vyv."),
    ]);

    let church_replacements =
        build_replacements(&[(r"^[Kk]ostol\b *", ""), (r"\b([Ss]vät\w+|Sv\.)", "sv.")]);

    let chapel_replacements =
        build_replacements(&[(r"^[Kk]aplnka\b *", ""), (r"\b([Ss]vät\w+|Sv\.)", "sv.")]);

    let school_replacements = build_replacements(&[
        (r"[Zz]ákladná [Šš]kola", "ZŠ"),
        (r"[Zz]ákladná [Uu]melecká [Šš]kola", "ZUŠ"),
        (r"[Ss]tredná [Oo]dborná [Šš]kola", "SOŠ"),
        (r"[Gg]ymnázium ", "gym. "),
        (r" [Gg]ymnázium", " gym."),
        (r"[V]ysoká [Šš]kola", "VŠ"),
    ]);

    let college_replacements = build_replacements(&[
        (r"[Ss]tredná [Oo]dborná [Šš]kola", "SOŠ"),
        (r"[Gg]ymnázium ", "gym. "),
        (r" [Gg]ymnázium", " gym."),
        (r"[V]ysoká [Šš]kola", "VŠ"),
    ]);

    let university_replacements = build_replacements(&[(r"[V]ysoká [Šš]kola", "VŠ")]);

    use Category::{
        Accommodation, Barrier, Culture, Facility, Finance, GastroPoi, Health, Historic,
        Institution, ManMade, NaturalPoi, Other, Railway, Religion, RoadsAndPaths, Shop, Sport,
        Terrain, Tourism, Transport, Water,
    };

    // The order of these entries IS the collision priority: the earlier a type appears,
    // the earlier its icons are drawn and the more likely they are to win contested pixels
    // (and to keep their exact coordinate - see OFFSETS). Zoom lives in each entry's own
    // fields, so priority and zoom are free to disagree, and deliberately do: an aerodrome
    // is drawn from z12 but sits low here, so that at z18 it yields to the town POIs around
    // it. Sort by importance to the reader, never by zoom.
    //
    // Types listed together must stay adjacent: `render_icons` picks the first definition
    // matching the zoom, so a narrower `max_zoom` variant has to precede the general one.
    // Cancels the muted grey a shop/civic category would otherwise give: this is
    // something a walker actually uses.
    let black = || Extra {
        color: Some(colors::POI_BLACK),
        ..Extra::default()
    };

    #[rustfmt::skip]
    let entries = vec![
        (14, 15, Y, N, Historic, "monument", Extra::default()),
        (14, 15, Y, N, Historic, "archaeological_site", Extra::default()),
        (14, 15, Y, N, Tourism, "tower_observation", Extra::default()),
        (14, 15, Y, N, Tourism, "tower_watchtower", Extra { icon: Some("tower_observation"), ..Extra::default() }),
        (14, 15, Y, Y, NaturalPoi, "cave_entrance", Extra {
            replacements: build_replacements(&[
                (r"^[Jj]jaskyňa\b *", ""),
                (r"\b[Jj]jaskyňa$", "j."),
                (r"\b[Pp]riepasť\b", "p."),
            ]),
            ..Extra::default()
        }),
        (14, 15, Y, Y, NaturalPoi, "arch", Extra::default()),
        // `information=office`, but imposm imports it as a bare `office`: mapping.yaml aliases
        // only `information=terminal`, so every other value arrives under its own name. Keyed
        // `information_office` this matched nothing and the 200-odd tourist information
        // offices in the data were never drawn.
        (15, 16, N, N, Tourism, "office", Extra { icon: Some("information_office"), ..Extra::default() }),
        (17, 18, N, N, Tourism, "information_terminal", Extra::default()),           // information=terminal
        (17, 18, N, N, Tourism, "audioguide", Extra::default()),           // information=audioguide
        (14, 15, N, N, Sport, "water_park", Extra::default()),
        (14, 15, Y, N, Accommodation, "hotel", Extra {
            replacements: build_replacements(&[(r"^[Hh]otel\b *", "")]),
            ..Extra::default()
        }),
        (14, 15, Y, N, Accommodation, "chalet", Extra {
            replacements: build_replacements(&[
                (r"^[Cc]hata\b *", ""),
                (r"\b[Cc]hata$", "ch."),
            ]),
            ..Extra::default()
        }),
        (14, 15, Y, N, Accommodation, "hostel", Extra::default()),
        (14, 15, Y, N, Accommodation, "motel", Extra {
            replacements: build_replacements(&[(r"^[Mm]otel\b *", "")]),
            ..Extra::default()
        }),
        (14, 15, Y, N, Accommodation, "guest_house", Extra::default()),
        (14, 15, Y, N, Accommodation, "alpine_hut", Extra::default()),
        (14, 15, Y, N, Accommodation, "apartment", Extra::default()),
        (14, 15, Y, N, Accommodation, "wilderness_hut", Extra::default()),
        (15, 16, Y, N, Accommodation, "basic_hut", Extra::default()),
        (14, 15, N, N, Accommodation, "caravan_site", Extra::default()),
        (14, 15, Y, N, Accommodation, "camp_site", Extra::default()),
        (14, 14, N, N, Historic, "castle", Extra {
            replacements: build_replacements(&[(r"^[Hh]rad\b *", "")]),
            ..Extra::default()
        }),
        (14, 15, N, N, Historic, "manor", Extra::default()),
        (14, 15, N, N, ManMade, "forester's_lodge", Extra::default()),
        // (12, 12, Y, N, "guidepost", Extra { icon: Some("guidepost_x"), weight: Weight::BOLD, max_zoom: 12, ..Extra::default() }),
        (13, 13, Y, N, RoadsAndPaths, "guidepost", Extra { icon: Some("guidepost_xx"), weight: Weight::BOLD, max_zoom: 13, ..Extra::default() }),
        (14, 14, Y, N, RoadsAndPaths, "guidepost", Extra { icon: Some("guidepost_xx"), weight: Weight::BOLD, ..Extra::default() }),
        (14, 15, N, N, Religion, "cathedral", Extra {
            replacements: church_replacements.clone(),
            icon: Some("church"),
            ..Extra::default()
        }),
        (14, 15, N, N, Religion, "church", Extra {
            replacements: church_replacements.clone(),
            ..Extra::default()
        }),
        (14, 15, N, N, Religion, "chapel", Extra::default()),
        (14, 15, N, N, Religion, "synagogue", Extra::default()),
        (14, 15, N, N, Religion, "mosque", Extra::default()),
        (14, 15, N, N, Religion, "temple", Extra { icon: Some("church"), ..Extra::default() }), // TODO no temple icon yet
        (14, 15, N, N, Railway, "station", Extra::default()),
        (14, 15, N, N, Railway, "halt", Extra { icon: Some("station"), ..Extra::default() }),
        (14, 15, N, N, Transport, "bus_station", Extra::default()),
        (14, 15, N, N, Culture, "museum", Extra::default()),
        (15, 16, N, N, Culture, "cinema", Extra {
            replacements: build_replacements(&[(r"^[Kk]ino\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Culture, "theatre", Extra {
            replacements: build_replacements(&[(r"^[Dd]ivadlo\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Sport, "climbing", Extra::default()),
        (14, 15, N, N, Sport, "free_flying", Extra::default()),
        (15, 16, N, N, Sport, "shooting", Extra::default()),
        (15, 16, N, N, Historic, "bunker", Extra::default()),
        (15, 16, N, N, Historic, "historic_bunker", Extra { icon: Some("bunker"), ..Extra::default() }),
        (15, 16, N, N, GastroPoi, "restaurant", Extra {
            replacements: build_replacements(&[(r"^[Rr]eštaurácia\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, GastroPoi, "pub", Extra::default()),
        (15, 16, N, N, GastroPoi, "biergarten", Extra::default()),
        // Black, not the shop grey: this is where a hiker eats. `greengrocer` must match
        // `farm` whatever else changes - they draw one icon.
        (15, 16, N, N, Shop, "farm", Extra { icon: Some("greengrocer"), color: Some(colors::POI_BLACK), ..Extra::default()}),
        (15, 16, N, N, Shop, "greengrocer", black()),
        (15, 16, N, N, Shop, "convenience", black()),
        (15, 16, N, N, Shop, "supermarket", black()),
        (15, 16, N, N, Transport, "fuel", Extra::default()),
        (15, 16, N, N, GastroPoi, "fast_food", Extra::default()),
        (15, 16, N, N, GastroPoi, "cafe", Extra {
            replacements: build_replacements(&[(r"^[Kk]aviareň\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, GastroPoi, "bar", Extra::default()),
        (15, 16, N, N, Shop, "pastry", Extra { icon: Some("confectionery"), ..black() }),
        (15, 16, N, N, Shop, "confectionery", black()),
        (16, 17, N, N, GastroPoi, "ice_cream", Extra::default()),
        (15, 16, N, N, Health, "pharmacy", Extra {
            replacements: build_replacements(&[(r"^[Ll]ekáreň\b *", "")]),
            ..Extra::default()
        }),
        (18, 19, N, N, Health, "dentist", Extra::default()),
        (17, 18, N, N, Health, "doctors", Extra::default()),
        (17, 18, N, N, Health, "clinic", Extra { icon: Some("doctors"), ..Extra::default() }),
        (18, 19, N, N, Health, "veterinary", Extra::default()),
        (17, 18, N, N, Transport, "bicycle_rental", Extra::default()),
        (17, 18, N, N, Transport, "bicycle_repair_station", Extra::default()),
        (17, 18, N, N, Transport, "car_rental", Extra::default()),
        (16, 17, N, N, Culture, "dance", Extra::default()),
        (16, 17, N, N, Historic, "city_gate", Extra::default()),
        (16, 17, N, N, Sport, "miniature_golf", Extra::default()),
        (16, 17, N, N, Sport, "leisure_miniature_golf", Extra { icon: Some("miniature_golf"), ..Extra::default() }),
        (16, 17, N, N, Sport, "cycling", Extra::default()),
        (16, 17, N, N, Sport, "soccer", Extra::default()),
        (16, 17, N, N, Sport, "tennis", Extra::default()),
        (16, 17, N, N, Sport, "basketball", Extra::default()),
        (16, 17, N, N, Sport, "volleyball", Extra::default()),
        (16, 17, N, N, Sport, "ice_skating", Extra::default()),
        (16, 17, N, N, Sport, "fitness_centre", Extra::default()),
        (16, 17, N, N, Sport, "fitness_station", Extra::default()),
        (16, 17, N, N, Sport, "running", Extra::default()),
        (16, 17, N, N, Sport, "athletics", Extra { icon: Some("running"), ..Extra::default() }),
        (16, 17, N, N, Sport, "swimming", Extra { icon: Some("water_park"), ..Extra::default() }),
        (16, 17, N, N, Sport, "bowling_alley", Extra::default()),
        (14, 15, Y, Y, Water, "waterfall", Extra {
            replacements: build_replacements(&[
                (r"^[Vv]odopád\b *", ""),
                (r"\b[Vv]odopád$", "vdp."),
            ]),
            text_color: Some(colors::WATER_LABEL),
            ..Extra::default()
        }),
        (15, 16, N, N, Water, "dam", Extra { text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (16, 17, N, N, Water, "weir", Extra { text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (16, NN, N, N, Water, "ford", Extra::default()),
        // Passability, like the ford above: these say whether you get through at all,
        // which outranks anything merely worth looking at.
        (14, NN, N, N, Terrain, "obstacle_tree", Extra::default()),
        (14, NN, N, N, Terrain, "obstacle_vegetation", Extra::default()),
        (14, NN, N, N, Terrain, "obstacle", Extra::default()),
        (14, 15, Y, Y, Water, "spring", Extra { replacements: spring_replacements.clone(), text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (14, 15, N, N, Water, "drinking_water", Extra { text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (14, 15, N, N, Water, "water_point", Extra { text_color: Some(colors::WATER_LABEL), icon: Some("drinking_water"), ..Extra::default() }),
        (14, 15, N, N, Water, "water_well", Extra { text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (15, 16, N, N, ManMade, "generator_wind", Extra::default()),
        (14, 15, Y, N, ManMade, "adit", Extra { icon: Some("mine"), ..Extra::default() }),
        (14, 15, Y, N, ManMade, "mineshaft", Extra { icon: Some("mine"), ..Extra::default() }),
        (15, 16, Y, N, Historic, "historic_mine", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (15, 16, Y, N, Historic, "mine_shaft", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (15, 16, Y, N, Historic, "mine_adit", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (15, 16, Y, N, Historic, "disused_adit", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (15, 16, Y, N, Historic, "disused_mineshaft", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (15, 16, Y, N, Historic, "abandoned_adit", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (15, 16, Y, N, Historic, "abandoned_mineshaft", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (14, 15, N, N, Institution, "townhall", Extra {
            replacements: chapel_replacements.clone(),
            ..Extra::default()
        }),
        (15, 16, N, N, Historic, "memorial", Extra {
            replacements: build_replacements(&[(r"^[Pp]amätník\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "university", Extra { replacements: university_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, Institution, "college", Extra { replacements: college_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, Institution, "school", Extra { replacements: school_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, Institution, "kindergarten", Extra {
            replacements: build_replacements(&[(r"[Mm]atersk(á|ou) [Šš]k[oô]lk?(a|ou)", "MŠ")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "community_centre", Extra {
            replacements: build_replacements(&[(r"\b[Cc]entrum voľného času\b", "CVČ")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "fire_station", Extra {
            replacements: build_replacements(&[(r"^([Hh]asičská zbrojnica|[Pp]ožiarná stanica)\b *", "")]),
            color: Some(colors::POI_BLACK), ..Extra::default()
        }),
        (15, 16, N, N, Institution, "police", Extra {
            replacements: build_replacements(&[(r"^[Pp]olícia\b *", "")]),
            color: Some(colors::POI_BLACK), ..Extra::default()
        }),
        (15, 16, N, N, Institution, "prison", Extra::default()),
        (15, 16, N, N, Institution, "courthouse", Extra::default()),
        (15, 16, N, N, Institution, "post_office", Extra::default()),
        (15, 16, N, N, Finance, "bank", Extra::default()),
        (17, 18, N, N, Finance, "atm", black()),
        (16, 17, N, N, Finance, "bureau_de_change", Extra::default()),
        (14, 15, N, N, Sport, "horse_racing", Extra { icon: Some("horse_riding"), ..Extra::default() }), // TODO use different icon
        (14, 15, N, N, Sport, "horse_riding", Extra::default()),
        (14, 15, N, N, Sport, "equestrian", Extra { icon: Some("horse_riding"), ..Extra::default() }),
        (16, 17, N, N, Sport, "leisure_horse_riding", Extra { icon: Some("horse_riding"), ..Extra::default() }),
        (15, 16, Y, N, Facility, "picnic_shelter", Extra::default()),
        (15, 16, Y, N, Accommodation, "weather_shelter", Extra::default()),
        (15, 16, Y, N, Accommodation, "shelter", Extra::default()),
        (15, 16, Y, N, Accommodation, "lean_to", Extra::default()),
        (15, 16, N, N, Other, "hunting_stand", Extra::default()),
        (15, 16, N, N, Other, "bird_hide", Extra::default()),
        (14, 15, Y, Y, Tourism, "viewpoint", Extra {
            replacements: build_replacements(&[
                (r"^[Vv]yhliadka\b *", ""),
                (r"\b[Vv]yhliadka$", "vyhl."),
            ]),
            ..Extra::default()
        }),
        (15, 16, N, N, Transport, "taxi", Extra::default()),
        (15, 16, N, N, Transport, "bus_stop", Extra::default()),
        (15, 16, N, N, Transport, "ferry_terminal", Extra::default()),
        (15, 16, Y, N, Transport, "public_transport", Extra::default()),
        (15, 16, N, N, Religion, "tower_bell_tower", Extra::default()),
        (15, 15, N, Y, NaturalPoi, "tree_protected", Extra { color: Some(colors::POI_TREE), text_color: Some(colors::TREE), ..Extra::default() }),
        (15, 16, N, N, Shop, "bicycle", black()),
        (16, NN, N, N, Facility, "toilets", Extra::default()),
        // A nameless board icon says nothing (it could be a nature panel, a notice board, a
        // timetable case), so the label must arrive with the icon - keep both at the same zoom.
        (17, 17, N, N, Tourism, "board", Extra::default()),
        (17, 18, N, N, Tourism, "map", Extra::default()),
        (16, 17, N, N, Culture, "artwork", Extra::default()),
        (16, 17, N, N, Water, "fountain", Extra { text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        // TODO (14, 14, N, N, "recycling", Extra { text_color: Some(colors::AREA_LABEL), ..Extra::default() }), // { icon: null } // has no icon yet - render as area name
        (16, 17, N, N, Sport, "playground", Extra {
            replacements: build_replacements(&[(r"^[Dd]etské ihrisko\b", "")]),
            ..Extra::default()
        }),
        (17, 18, N, N, Religion, "wayside_shrine", Extra::default()),
        (16, 17, N, N, Religion, "cross", Extra::default()),
        (17, 18, N, N, Religion, "wayside_cross", Extra { icon: Some("cross"), ..Extra::default() }), // NOTE cross is also on lower zoom
        (17, 18, N, N, Religion, "tree_shrine", Extra { icon: Some("cross"), ..Extra::default() }), // NOTE cross is also on lower zoom
        (16, 17, N, Y, NaturalPoi, "rock", Extra::default()),
        (16, 17, N, Y, NaturalPoi, "stone", Extra::default()),
        (16, 17, N, Y, NaturalPoi, "sinkhole", Extra::default()),
        (18, 19, N, N, Facility, "post_box", Extra::default()),
        (18, 19, N, N, Facility, "parcel_locker", Extra::default()),
        (18, 19, N, N, Facility, "telephone", Extra::default()),
        (18, 19, N, N, Facility, "phone", Extra::default()),
        (15, 16, N, N, ManMade, "chimney", Extra::default()),
        (15, 16, N, N, ManMade, "water_tower", Extra::default()),
        (14, 15, N, N, Tourism, "attraction", Extra::default()),

        (15, 16, N, N, Shop, "marketplace", black()),
        (15, 16, N, N, Sport, "public_bath", Extra::default()),
        (15, 16, N, N, Sport, "fishing", Extra::default()),
        (15, 16, N, N, Transport, "helipad", Extra::default()),
        (15, 16, N, N, Transport, "charging_station", Extra::default()),
        (15, 16, N, N, Historic, "tower_defensive", Extra::default()),
        (15, 16, N, N, ManMade, "tower_cooling", Extra::default()),
        (15, 16, N, N, ManMade, "cooling_tower", Extra { icon: Some("tower_cooling"), ..Extra::default() }),
        (15, 16, N, N, ManMade, "windmill", Extra::default()),
        (15, 16, N, N, ManMade, "lighthouse", Extra::default()),
        (15, 16, N, N, Historic, "obelisk", Extra::default()),
        (15, 16, N, N, Culture, "casino", Extra::default()),
        (15, 16, N, N, Culture, "gallery", Extra::default()),
        (15, 16, N, N, Culture, "arts_centre", Extra::default()),
        (15, 16, N, N, Culture, "nightclub", Extra::default()),
        (15, 16, N, N, Sport, "sauna", Extra::default()),
        (16, 17, N, N, Sport, "massage", Extra::default()),
        (17, 18, N, N, Facility, "shower", Extra::default()),
        (15, NN, N, N, ManMade, "tower_communication", Extra::default()),
        (15, NN, N, N, ManMade, "communications_tower", Extra { icon: Some("tower_communication"), ..Extra::default() }),
        (15, NN, N, N, ManMade, "mast_communication", Extra { icon: Some("tower_communication"), ..Extra::default() }),
        // Plain towers and masts: the old list ranked "tower_other"/"mast_other", names the
        // query never emits, so both sank to the bottom instead of ranking here.
        (15, NN, N, N, ManMade, "tower", Extra::default()),
        (15, NN, N, N, ManMade, "mast", Extra::default()),
        (10, 10, Y, Y, NaturalPoi, "volcano", Extra { icon: Some("peak"), font_size: 13.0, halo: false, text_color: Some(colors::MILITARY), color: Some(colors::POI_VOLCANO), ..Extra::default() }),
        (10, 10, Y, Y, NaturalPoi, "peak1", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (11, 11, Y, Y, NaturalPoi, "peak2", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (12, 12, Y, Y, NaturalPoi, "peak3", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (13, 13, Y, Y, NaturalPoi, "peak", Extra { font_size: 13.0, halo: false, ..Extra::default() }),
        (15, 15, Y, Y, NaturalPoi, "saddle", Extra { font_size: 13.0, halo: false, ..Extra::default() }),
        (15, 15, Y, Y, NaturalPoi, "mountain_pass", Extra { icon: Some("saddle"), font_size: 13.0, halo: false, ..Extra::default() }),
        (16, 17, N, N, Water, "water_works", Extra { text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (16, 17, N, N, Water, "reservoir_covered", Extra { icon: Some("water_works"), text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (16, 17, N, N, Water, "pumping_station", Extra { icon: Some("water_works"), text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (16, 17, N, N, Water, "wastewater_plant", Extra { icon: Some("water_works"), text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (16, 17, N, N, ManMade, "storage_tank", Extra::default()),
        (16, 17, N, N, ManMade, "silo", Extra { icon: Some("storage_tank"), ..Extra::default() }),
        (16, NN, N, N, Facility, "firepit", Extra::default()),
        (16, NN, N, N, Facility, "outdoor_seating", Extra::default()),
        (16, NN, N, N, Facility, "picnic_table", Extra::default()),
        (16, 17, N, N, Facility, "bbq", Extra::default()),
        (17, 19, N, N, Transport, "parking", Extra { font_size: 10.0, text_color: Some(colors::AREA_LABEL), ..Extra::default() }), // { font: { haloOpacity: 0.5 } },
        (18, NN, N, N, Facility, "bench", Extra::default()),
        (17, 18, N, N, Other, "beehive", Extra::default()),
        (17, 18, N, N, Other, "apiary", Extra { icon: Some("beehive"), ..Extra::default() }),
        (17, 18, N, N, Historic, "boundary_stone", Extra::default()),
        (17, 18, N, N, Historic, "marker", Extra { icon: Some("boundary_stone"), ..Extra::default() }),
        (16, NN, N, N, Water, "watering_place", Extra { text_color: Some(colors::WATER_LABEL), ..Extra::default() }),
        (17, NN, N, N, Barrier, "lift_gate", Extra::default()),
        (17, NN, N, N, Barrier, "swing_gate", Extra { icon: Some("lift_gate"), ..Extra::default() }),
        (17, NN, N, N, Barrier, "motorcycle_barrier", Extra::default()),
        (17, NN, N, N, Barrier, "full-height_turnstile", Extra::default()),
        (17, NN, N, N, Barrier, "kissing_gate", Extra::default()),
        (17, NN, N, N, Barrier, "cattle_grid", Extra::default()),
        (17, NN, N, N, Barrier, "toll_booth", Extra::default()),
        (17, NN, N, N, Barrier, "stile", Extra::default()),
        (17, NN, N, N, Barrier, "cycle_barrier", Extra::default()),
        (19, NN, N, N, Barrier, "bollard", Extra::default()),
        (19, NN, N, N, Barrier, "block",  Extra { icon: Some("bollard"), ..Extra::default() }),
        (19, NN, N, N, Barrier, "turnstile",  Extra { icon: Some("bollard"), ..Extra::default() }),
        (19, NN, N, N, Barrier, "log",  Extra { icon: Some("bollard"), ..Extra::default() }),
        (18, NN, N, N, Facility, "waste_disposal", Extra::default()),
        (19, NN, N, N, Facility, "waste_basket", Extra::default()),
        (16, NN, N, N, Other, "feeding_place", Extra { icon: Some("manger"), ..Extra::default() }),
        (16, NN, N, N, Other, "game_feeding", Extra { icon: Some("manger"), ..Extra::default() }),
        (15, 16, N, N, Historic, "ruins", Extra::default()),
        (16, 17, N, N, Other, "building", Extra::default()),
        (18, 19, N, N, Historic, "building_ruins", Extra { icon: Some("ruins"), ..Extra::default() }),
        (15, 15, N, Y, NaturalPoi, "tree", Extra { color: Some(colors::POI_TREE), text_color: Some(colors::TREE), ..Extra::default() }),
        (18, NN, N, N, Barrier, "gate", Extra::default()),
        (15, NN, Y, N, RoadsAndPaths, "guidepost_noname", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (16, NN, Y, N, RoadsAndPaths, "route_marker", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (14, 15, N, N, Sport, "skiing", Extra::default()),
        (14, 15, N, N, Health, "hospital", Extra {
            replacements: build_replacements(&[(r"^[Nn]emocnica\b", "Nem.")]),
            ..Extra::default()
        }),
        (14, 15, N, N, Sport, "golf_course", Extra::default()),
        (14, 15, N, N, Sport, "beach_resort", Extra::default()),
        (15, 16, N, N, Facility, "picnic_site", Extra::default()),
        (12, 12, N, N, Transport, "aerodrome", Extra {
            replacements: build_replacements(&[(r"^[Ll]etisko\b *", "")]),
            ..Extra::default()
        }),

        // Shops and services, from OpenStreetMap Carto's symbols. Carto draws nearly all
        // from z18; here they sit one or two zooms deeper, tiered by worth to an outdoor
        // reader, and rank last so a jeweller never takes a pixel from a spring.
        //
        // `min_text_zoom` is `min_zoom + 1` except in the deepest tier, where there is no
        // zoom left to wait for and a nameless shop icon says almost nothing.

        // Resupply, repair and landmark stores.
        (18, 19, N, N, Shop, "bakery", black()),
        (18, 19, N, N, Shop, "butcher", black()),
        (18, 19, N, N, Shop, "chemist", Extra::default()),
        (18, 19, N, N, Shop, "alcohol", Extra::default()),
        (18, 19, N, N, Shop, "wine", Extra { icon: Some("alcohol"), ..Extra::default() }),
        (18, 19, N, N, Shop, "beverages", black()),
        (18, 19, N, N, Shop, "newsagent", Extra::default()),
        (18, 19, N, N, Shop, "kiosk", Extra { icon: Some("newsagent"), ..Extra::default() }),
        (18, 19, N, N, Shop, "deli", black()),
        (18, 19, N, N, Shop, "department_store", Extra::default()),
        (18, 19, N, N, Shop, "outdoor", black()),
        (18, 19, N, N, Shop, "sports", black()),
        (18, 19, N, N, Shop, "doityourself", black()),
        (18, 19, N, N, Shop, "hardware", Extra { icon: Some("doityourself"), ..black() }),
        (18, 19, N, N, Shop, "car_repair", Extra::default()),

        // Ordinary high-street shops.
        (19, 20, N, N, Shop, "clothes", Extra::default()),
        (19, 20, N, N, Shop, "fashion", Extra { icon: Some("clothes"), ..Extra::default() }),
        (19, 20, N, N, Shop, "shoes", Extra::default()),
        (19, 20, N, N, Shop, "hairdresser", Extra::default()),
        (19, 20, N, N, Shop, "beauty", Extra::default()),
        (19, 20, N, N, Shop, "perfumery", Extra::default()),
        (19, 20, N, N, Shop, "cosmetics", Extra { icon: Some("perfumery"), ..Extra::default() }),
        (19, 20, N, N, Shop, "optician", Extra::default()),
        (19, 20, N, N, Shop, "books", Extra::default()),
        (19, 20, N, N, Shop, "gift", black()),
        (19, 20, N, N, Shop, "toys", Extra::default()),
        (19, 20, N, N, Shop, "stationery", Extra::default()),
        (19, 20, N, N, Shop, "pet", Extra::default()),
        (19, 20, N, N, Shop, "florist", Extra::default()),
        (19, 20, N, N, Shop, "dairy", black()),
        (19, 20, N, N, Shop, "seafood", black()),
        (19, 20, N, N, Shop, "fishmonger", Extra { icon: Some("seafood"), ..black() }),
        (19, 20, N, N, Shop, "coffee", Extra::default()),
        (19, 20, N, N, Shop, "tea", Extra::default()),
        (19, 20, N, N, Shop, "tobacco", Extra::default()),
        (19, 20, N, N, Shop, "electronics", Extra::default()),
        (19, 20, N, N, Shop, "computer", Extra::default()),
        (19, 20, N, N, Shop, "mobile_phone", Extra::default()),
        (19, 20, N, N, Shop, "photo", Extra::default()),
        (19, 20, N, N, Shop, "photo_studio", Extra { icon: Some("photo"), ..Extra::default() }),
        (19, 20, N, N, Shop, "photography", Extra { icon: Some("photo"), ..Extra::default() }),
        (19, 20, N, N, Shop, "travel_agency", Extra::default()),
        (19, 20, N, N, Shop, "ticket", Extra::default()),
        (19, 20, N, N, Shop, "variety_store", Extra::default()),
        (19, 20, N, N, Shop, "laundry", Extra::default()),
        (19, 20, N, N, Shop, "dry_cleaning", Extra { icon: Some("laundry"), ..Extra::default() }),
        (19, 20, N, N, Shop, "garden_centre", Extra::default()),
        (19, 20, N, N, Shop, "car", Extra::default()),
        (19, 20, N, N, Shop, "motorcycle", Extra::default()),
        (19, 20, N, N, Shop, "car_wash", Extra::default()),

        // The rest, held to the deepest zoom.
        (20, 20, N, N, Shop, "furniture", Extra::default()),
        (20, 20, N, N, Shop, "interior_decoration", Extra::default()),
        (20, 20, N, N, Shop, "carpet", Extra::default()),
        (20, 20, N, N, Shop, "fabric", Extra::default()),
        (20, 20, N, N, Shop, "bed", Extra::default()),
        (20, 20, N, N, Shop, "houseware", Extra::default()),
        (20, 20, N, N, Shop, "paint", Extra::default()),
        (20, 20, N, N, Shop, "art", Extra::default()),
        (20, 20, N, N, Shop, "bag", Extra::default()),
        (20, 20, N, N, Shop, "jewelry", Extra::default()),
        (20, 20, N, N, Shop, "music", Extra::default()),
        (20, 20, N, N, Shop, "musical_instrument", Extra::default()),
        (20, 20, N, N, Shop, "hifi", Extra::default()),
        (20, 20, N, N, Shop, "video", Extra::default()),
        (20, 20, N, N, Shop, "video_games", Extra::default()),
        (20, 20, N, N, Shop, "bookmaker", Extra::default()),
        (20, 20, N, N, Shop, "charity", Extra::default()),
        (20, 20, N, N, Shop, "second_hand", Extra::default()),
        (20, 20, N, N, Shop, "copyshop", Extra::default()),
        (20, 20, N, N, Shop, "hearing_aids", Extra::default()),
        (20, 20, N, N, Shop, "medical_supply", Extra::default()),
        (20, 20, N, N, Shop, "car_parts", Extra::default()),
        (20, 20, N, N, Shop, "tyres", Extra::default()),
        (20, 20, N, N, Shop, "motorcycle_repair", Extra::default()),
        (20, 20, N, N, Shop, "vehicle_inspection", Extra::default()),
        (20, 20, N, N, Shop, "trade", Extra::default()),
        (20, 20, N, N, Shop, "wholesale", Extra { icon: Some("trade"), ..Extra::default() }),

        // Spot heights: summits and saddles with no name, labelled with their bare
        // elevation. Last in the list on purpose - the rank is the position here, so
        // these are the first thing collision drops, never crowding out a named summit
        // or any other POI. An elevation-less summit reaches `peak_noname` too, but only
        // from z16 (see `query`), and renders as a marker with no label at all - the
        // label is skipped for want of anything to put in it, not by `min_text_zoom`.
        //
        // 10.4 = 13.0 * the 0.8 `sub_size_scale` render_labels applies to the elevation
        // under a named summit. The elevation is the whole label here, so it lands on
        // line 0 and is never scaled - stating the product keeps a spot height from
        // reading as *louder* than the named summit it sits next to.
        (14, 14, Y, Y, NaturalPoi, "peak_noname", Extra { ele_only_label: true, icon: Some("peak"), font_size: 10.4, halo: false, ..Extra::default() }),
        (14, 14, Y, Y, NaturalPoi, "volcano_noname", Extra { ele_only_label: true, icon: Some("peak"), font_size: 10.4, halo: false, text_color: Some(colors::MILITARY), color: Some(colors::POI_VOLCANO), ..Extra::default() }),
        (15, 15, Y, Y, NaturalPoi, "saddle_noname", Extra { ele_only_label: true, icon: Some("saddle"), font_size: 10.4, halo: false, ..Extra::default() }),
        (15, 15, Y, Y, NaturalPoi, "mountain_pass_noname", Extra { ele_only_label: true, icon: Some("saddle"), font_size: 10.4, halo: false, ..Extra::default() }),
    ];

    entries
});

/// The `shop=*` values the POI layer draws, i.e. what the `osm_shops` query may select.
/// `mapping.yaml` imports `shop: __any__`, so the table holds every shop in the extract
/// and this is what narrows it; the definition in [`POI_ENTRIES`] still decides the zoom.
pub static SHOP_TYPES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    // `Shop` is the legend grouping, which is not quite the set of `shop=*` tags: these
    // three are amenities that arrive via `osm_pois`, and `shop=massage` is filed under
    // Sport because that is where a reader looks for it.
    const FROM_OSM_POIS: [&str; 3] = ["marketplace", "car_wash", "vehicle_inspection"];
    const ALSO: [&str; 1] = ["massage"];

    POI_ENTRIES
        .iter()
        .filter(|(_, _, _, _, category, typ, _)| {
            *category == Category::Shop && !FROM_OSM_POIS.contains(typ)
        })
        .map(|(_, _, _, _, _, typ, _)| *typ)
        .chain(ALSO)
        .collect()
});

pub static POIS: LazyLock<HashMap<&'static str, Vec<Def>>> = LazyLock::new(|| {
    let mut pois = HashMap::new();

    for (min_zoom, min_text_zoom, with_ele, natural, category, name, extra) in POI_ENTRIES.iter() {
        pois.entry(*name).or_insert_with(Vec::new).push(Def {
            min_zoom: *min_zoom,
            min_text_zoom: *min_text_zoom,
            with_ele: *with_ele,
            natural: *natural,
            category: *category,
            extra: extra.clone(),
        });
    }

    pois
});

pub static POI_ORDER: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut order = Vec::new();
    let mut seen = HashSet::new();

    for (_, _, _, _, _, name, _) in POI_ENTRIES.iter() {
        if seen.insert(*name) {
            order.push(*name);
        }
    }

    order
});

/// Builds the SQL `CASE` that ranks POI types, lowest rank drawn first.
///
/// The rank is a type's position in [`POI_ORDER`], i.e. where its first definition
/// appears in `POI_ENTRIES`. Deriving it from the definitions rather than from a
/// list of its own means every rendered type has a rank by construction, and no
/// rank can name a type that no query produces.
fn build_poi_z_order_case(column: &str) -> String {
    let mut case = format!("CASE {column}");

    for (idx, typ) in POI_ORDER.iter().enumerate() {
        let escaped = typ.replace('\'', "''");
        let _ = write!(case, " WHEN '{escaped}' THEN {idx}");
    }

    case.push_str(" END");

    case
}

/// Built once: the `CASE` is the same text for every tile.
static POI_Z_ORDER_CASE: LazyLock<String> = LazyLock::new(|| build_poi_z_order_case("type"));

/// Whether any definition of `typ` is drawn at `zoom`.
fn drawn_at(typ: &str, zoom: u8) -> bool {
    POIS.get(typ)
        .is_some_and(|defs| defs.iter().any(|def| def.is_active_at(zoom)))
}

const RADII: [f64; 4] = [2.0, 4.0, 6.0, 8.0];

const fn offset_at(r: f64, idx: usize) -> (f64, f64) {
    let d = r * f64::consts::FRAC_1_SQRT_2;

    match idx {
        0 => (0.0, r),
        1 => (0.0, -r),
        2 => (r, 0.0),
        3 => (-r, 0.0),
        4 => (d, d),
        5 => (-d, d),
        6 => (d, -d),
        _ => (-d, -d),
    }
}

static OFFSETS: LazyLock<[(f64, f64); 33]> = LazyLock::new(|| {
    let mut offsets = [(0.0, 0.0); 33];
    let mut idx = 1;

    for &r in &RADII {
        for pos in 0..8 {
            offsets[idx] = offset_at(r, pos);
            idx += 1;
        }
    }

    offsets
});

pub async fn query(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
    kst_only: bool,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let zoom = ctx.zoom;

    // A spot height is labelled with nothing but its elevation, so an `ele` that is not
    // a plain number would render as junk with no name beside it to make sense of it.
    // Same shape as the ORDER BY at the bottom of this function.
    const NUMERIC_ELE: &str = r"(tags->'ele') ~ '^\s*-?\d+(\.\d+)?\s*$'";

    // A saddle or pass with neither a name nor a `ref` - a spot height, like the summits
    // above. Three places in the z14 query have to agree on this: the type it is given,
    // whether its `ele` is sanitised, and whether it is selected at all.
    const UNNAMED_SADDLE: &str = "type IN ('saddle', 'mountain_pass') AND \
        COALESCE(NULLIF(name, ''), tags->'ref', '') = ''";

    let mut selects = vec![];

    // TODO add hiking-only
    let kst_cond = if kst_only {
        r"AND (type <> 'guidepost' OR tags->'operator' ~* '\ykst\y|\ytanap\y')"
    } else {
        ""
    };

    selects.push(
        "SELECT
            osm_id,
            geometry,
            name,
            hstore(ARRAY['ele', tags->'ele', 'isolation', tags->'isolation']) AS extra,
            CASE WHEN type = 'volcano' THEN type
                WHEN isolation > 4500 THEN 'peak1'
                WHEN isolation BETWEEN 3000 AND 4500 THEN 'peak2'
                WHEN isolation BETWEEN 1500 AND 3000 THEN 'peak3'
                ELSE 'peak'
            END AS type
        FROM
            osm_pois
        NATURAL LEFT JOIN
            isolations
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            type IN ('peak', 'volcano') AND
            name <> ''
        ",
    );

    let noname_peak_sql;

    if zoom >= 14 {
        // Up to z15 a spot height has to carry an elevation, since that is its whole
        // label and there is no name beside it to make the marker worth the space. From
        // z16 the summit is drawn even without one - the bare marker still says "there
        // is a summit here" - and it costs nothing, because the ORDER BY below puts a
        // NULL `ele` behind every real one, so these are the first to lose to collision.
        let ele_cond = if zoom >= 16 {
            String::new()
        } else {
            format!("AND {NUMERIC_ELE}")
        };

        noname_peak_sql = format!(
            "SELECT
                osm_id,
                geometry,
                name,
                -- Nulled out unless it is a plain number: an `ele` like '1234 m' or
                -- 'summit' would be the entire label, with nothing to make sense of it.
                hstore('ele', CASE WHEN {NUMERIC_ELE} THEN tags->'ele' END) AS extra,
                type || '_noname' AS type
            FROM
                osm_pois
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                type IN ('peak', 'volcano') AND
                name = ''
                {ele_cond}
            "
        );

        selects.push(&noname_peak_sql);
    }

    let gte_z13_sql;

    if zoom >= 13 {
        gte_z13_sql = format!(
            "SELECT
                osm_id,
                geometry,
                name,
                hstore('ele', tags->'ele') AS extra,
                CASE WHEN type = 'guidepost' AND name = '' THEN 'guidepost_noname' ELSE type END
            FROM
                osm_pois
            WHERE
                type = 'guidepost' AND
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
                {kst_cond}
            "
        );

        selects.push(&gte_z13_sql);
    }

    if (12..=13).contains(&zoom) {
        selects.push(
            "SELECT
                osm_id,
                geometry,
                name,
                hstore('ele', tags->'ele') AS extra,
                type
            FROM
                osm_pois
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                type = 'aerodrome' AND
                tags ? 'icao'
            ",
        );
    }

    let z14_sql;

    if zoom >= 14 {
        let w = {
            let mut omit_types = vec!["'peak'".to_string(), "'volcano'".to_string()];

            for (typ, defs) in POIS.iter() {
                let visible = defs
                    .iter()
                    .any(|def| def.min_zoom <= zoom && def.extra.max_zoom >= zoom);

                if !visible {
                    omit_types.push(format!("'{typ}'"));
                }
            }

            format!("AND type NOT IN ({})", omit_types.join(", "))
        };

        // Unnamed saddles and passes are spot heights like the summits above, and follow
        // the same rule: up to z15 they need an elevation to be worth drawing, from z16
        // the marker alone earns its place.
        let noname_saddle_cond = if zoom >= 16 {
            String::new()
        } else {
            format!("AND (NOT ({UNNAMED_SADDLE}) OR {NUMERIC_ELE})")
        };

        // Italian route markers (segnavia) are mapped so densely - one node per painted
        // blaze along a trail, some 20 m apart - that at the zoom the type is otherwise
        // drawn from they bury the rest of the map. So in Italy they are held back to
        // the deepest zoom, where the trail is magnified enough to carry one marker per
        // blaze; everywhere else `route_marker` means a sparse, signpost-like object and
        // keeps its own zoom. Nothing else is suppressed by country here, so the rule is
        // spelled out rather than configured. Needs the `countries` table (see
        // sql/countries.sql), and is added only at the zooms in between: outside them
        // either `{w}` has already omitted the type or the markers are wanted, so the
        // lookup would be dead weight.
        const IT_ROUTE_MARKER_MIN_ZOOM: u8 = 20;

        let route_marker_cond = if zoom < IT_ROUTE_MARKER_MIN_ZOOM && drawn_at("route_marker", zoom)
        {
            "AND (
                type <> 'route_marker' OR
                NOT EXISTS (
                    SELECT 1
                    FROM countries c
                    WHERE
                        c.country = 'it' AND
                        c.geometry && osm_pois.geometry AND
                        ST_Intersects(c.geometry, osm_pois.geometry)
                )
            )"
        } else {
            ""
        };

        z14_sql = format!(
            "
        SELECT
            osm_id,
            geometry,
            CASE
                WHEN
                    type = 'board'
                    AND name <> ''
                    AND tags ? 'ref'
                    AND NOT (tags->'ref' = ANY(regexp_split_to_array(name, '[^[:alnum:]_]+')))
                    THEN tags->'ref' || '. ' || name
                ELSE COALESCE(NULLIF(name, ''), tags->'ref', '') END AS name,
            hstore(ARRAY[
                -- An unnamed saddle is labelled with its elevation and nothing else, so
                -- a value that is not a plain number is dropped rather than shown bare.
                -- Every other type keeps whatever it is tagged with: there the elevation
                -- is a second line under a name that already carries the feature.
                'ele', CASE
                    WHEN {UNNAMED_SADDLE} AND NOT ({NUMERIC_ELE}) THEN NULL
                    ELSE tags->'ele'
                END,
                'access', tags->'access',
                'hot', (type = 'hot_spring')::text,
                'drinkable', tags->'drinking_water',
                'refitted', tags->'refitted',
                'intermittent', COALESCE(tags->'intermittent', tags->'seasonal'),
                'water_characteristic', tags->'water_characteristic',
                'ruins', tags->'ruins'
            ]) AS extra,
            CASE
                WHEN
                    type = 'guidepost' AND
                    name = ''
                THEN 'guidepost_noname'
                WHEN {UNNAMED_SADDLE}
                THEN type || '_noname'
                WHEN
                    type = 'tree' AND
                    tags->'protected' <> 'no'
                THEN 'tree_protected'
                WHEN
                    type = 'shelter' AND
                    tags->'shelter_type' IN (
                        -- excluded on purpose: trolley bays are not shelters. There is no
                        -- 'shopping_cart' definition, so naming it here drops the shelter
                        -- instead of drawing one.
                        'shopping_cart', 'lean_to', 'public_transport', 'picnic_shelter',
                        'basic_hut', 'weather_shelter'
                    )
                THEN tags->'shelter_type'
                WHEN
                    type IN ('adit', 'mineshaft') AND
                    tags->'disused' <> 'no'
                THEN 'disused_' || type
                WHEN type IN ('hot_spring', 'geyser', 'spring_box')
                THEN 'spring'
                -- Only suffixes that have a definition of their own. A mast tagged
                -- tower:type=observation or bell_tower is incoherent - the primary tag
                -- says mast - and synthesising a name no definition matches would make
                -- render_icons skip the mast entirely instead of drawing a plain one.
                WHEN type = 'mast'
                THEN
                    'mast' || CASE tags->'tower:type'
                        WHEN 'communication' THEN '_communication'
                        ELSE ''
                    END
                WHEN type = 'tower'
                THEN
                    'tower' || CASE tags->'tower:type'
                        WHEN 'communication' THEN '_communication'
                        WHEN 'observation' THEN '_observation'
                        WHEN 'watchtower' THEN '_watchtower'
                        WHEN 'bell_tower' THEN '_bell_tower'
                        WHEN 'cooling' THEN '_cooling'
                        WHEN 'defensive' THEN '_defensive'
                        ELSE ''
                    END
                WHEN type IN ('obstacle_tree', 'obstacle_vegetation')
                THEN type
                WHEN type LIKE 'obstacle_%'
                THEN 'obstacle'
                ELSE type
            END AS type
        FROM
            osm_pois
        WHERE
            geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
            (
                type <> 'saddle' OR
                NOT EXISTS (
                    SELECT 1
                    FROM osm_pois b
                    WHERE
                        type = 'mountain_pass' AND
                        osm_pois.osm_id = b.osm_id
                )
            ) AND
            (
                type <> 'tree' OR
                tags->'protected' NOT IN ('', 'no') OR
                tags->'denotation' = 'natural_monument'
            )
            {noname_saddle_cond} {route_marker_cond} {w} {kst_cond}
        "
        );

        selects.push(&z14_sql);

        // TODO filter only used sports
        selects.push("
            SELECT
                osm_id,
                geometry,
                name,
                hstore(ARRAY[
                    'access', tags->'access'
                ]) AS extra,
                type
            FROM
                osm_sports
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                osm_id NOT IN (SELECT osm_id FROM osm_pois WHERE type IN ('leisure_miniature_golf', 'leisure_horse_riding'))
        ");

        selects.push(
            "
            SELECT
                osm_id,
                geometry,
                name,
                hstore('') as extra,
                building AS type
            FROM
                osm_place_of_worships
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                building IN ('chapel', 'church', 'temple', 'mosque', 'cathedral', 'synagogue')
        ",
        );
    }

    let shops_sql;

    if zoom >= 15 {
        selects.push(
            "
            SELECT
                osm_id,
                ST_PointOnSurface(geometry) AS geometry,
                name,
                hstore('') AS extra,
                'generator_wind' AS type
            FROM
                osm_power_generators
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                (source = 'wind' OR method = 'wind_turbine')
        ",
        );

        // Narrowed to the types drawn at this zoom, which keeps the deepest tier of
        // shops - most of them - out of the z15 result set entirely.
        let shop_types = SHOP_TYPES
            .iter()
            .filter(|typ| drawn_at(typ, zoom))
            .map(|typ| format!("'{typ}'"))
            .collect::<Vec<_>>();

        if !shop_types.is_empty() {
            shops_sql = format!(
                "
            SELECT
                osm_id,
                geometry,
                name,
                hstore('') AS extra,
                type
            FROM
                osm_shops
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                type IN ({})
        ",
                shop_types.join(", ")
            );

            selects.push(&shops_sql);
        }

        selects.push(
            "
            SELECT
                osm_id,
                ST_LineInterpolatePoint(geometry, 0.5) AS geometry,
                name,
                hstore('') AS extra,
                type
            FROM
                osm_feature_lines
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                type IN ('dam', 'weir', 'ford')
        ",
        );
    }

    let z_order_case = &*POI_Z_ORDER_CASE;

    let sql = format!(
        r"
        SELECT
            *
        FROM
            ({}) AS tmp
        ORDER BY
            {z_order_case},
            extra->'isolation' DESC NULLS LAST,
            CASE
                WHEN (extra->'ele') ~ '^\s*-?\d+(\.\d+)?\s*$' THEN (extra->'ele')::real
                ELSE NULL
            END DESC NULLS LAST,
            osm_id
        ",
        selects.join(" UNION ALL ")
    );

    drop(selects);

    client
        .query(&sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
        .await
}

pub(super) struct PendingLabel {
    point: Point,
    icon_half_height: f64,
    name: String,
    ele: Option<String>,
    bbox_idx: usize,
    alpha: f64,
    color: Color,
    halo_color: Option<Color>,
    def: &'static Def,
}

pub(super) type ToLabel = Vec<PendingLabel>;

/// The icon files and stylesheet for a `spring` POI, whose icon varies with several
/// `extra` tags (mineral/refitted/hot/intermittent/drinkable) rather than coming from a
/// single static definition.
fn spring_variant(extra: &HashMap<String, Option<String>>) -> (Vec<String>, Option<String>) {
    let mut stylesheet = String::new();

    let is_mineral = extra
        .get("water_characteristic")
        .is_some_and(|v| v.is_some() && v.as_deref() != Some(""));

    let mut names = vec![
        (if is_mineral {
            "mineral-spring"
        } else {
            "spring"
        })
        .to_string(),
    ];

    if !is_mineral
        && extra
            .get("refitted")
            .is_some_and(|r| r.as_deref() == Some("yes"))
    {
        names.push("refitted_spring".into());
    }

    let fill = if extra
        .get("hot")
        .is_some_and(|r| r.as_deref() == Some("true"))
    {
        "#e11919"
    } else {
        &colors::rgb_hex(colors::POI_WATER)
    };

    if extra
        .get("intermittent")
        .is_some_and(|r| r.as_deref() == Some("yes"))
    {
        names.push("intermittent".into());
    }

    let _ = write!(stylesheet, "#spring {{ fill: {fill} }}");

    match extra.get("drinkable").and_then(Option::as_deref) {
        Some("yes" | "treated") => {
            names.push("drinkable_spring".into());
            stylesheet.push_str(r"#drinkable { fill: #00ff00 } ");
        }
        Some("no") => {
            names.push("drinkable_spring".into());
            stylesheet.push_str(r"#drinkable { fill: #ff0000 } ");
        }
        _ => {}
    }

    (names, Some(stylesheet))
}

pub fn render_icons(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    collision: &mut Collision,
    svg_repo: &mut SvgRepo,
) -> Result<ToLabel, LayerRenderError> {
    let _span = tracy_client::span!("pois::render_icons");

    let zoom = ctx.zoom;

    let mut to_label = ToLabel::new();

    for row in rows {
        let typ = row.get_string("type")?;

        let extra = row.get_hstore("extra")?;

        let Some(def) = POIS.get(typ).and_then(|defs| {
            defs.iter()
                .find(|def| def.min_zoom <= zoom && def.extra.max_zoom >= zoom)
        }) else {
            continue;
        };

        let point = row.get_point()?.project_to_tile(&ctx.tile_projector);

        // `ruins=*` swaps the icon for the generic ruins one - the same one
        // `historic=ruins` draws - the way `building=* + ruins=yes` becomes a ruined
        // building in the buildings layer. Only the icon: the POI keeps its own
        // definition, so it is still labelled at its own zoom, with its own name
        // abbreviations and elevation, rather than being demoted to a bare `ruins`.
        let is_ruins = extra
            .get("ruins")
            .and_then(Option::as_deref)
            .is_some_and(|v| !matches!(v, "" | "no"));

        let key = if is_ruins { "ruins" } else { def.icon_key(typ) };

        // The colour follows the swapped icon: a ruined chalet drawn in the accommodation
        // violet would put the same glyph on one tile in two colours, and the legend only
        // ever shows the Historic one.
        let color = if is_ruins {
            Category::Historic.icon_color()
        } else {
            def.color()
        };

        let restricted = extra
            .get("access")
            .is_some_and(|access| matches!(access.as_deref(), Some("private" | "no")));

        let halo_color = (restricted && def.extra.halo).then_some(colors::ACCESS_RESTRICTED);

        let fade = if restricted && def.category.fades_when_restricted() {
            0.66
        } else {
            1.0
        };

        let (names, stylesheet) = if key == "spring" {
            spring_variant(&extra)
        } else {
            let fill = colors::rgb_hex(color);

            (
                vec![key.to_string()],
                Some(format!("path {{ fill: {fill} }}")),
            )
        };

        let surface = svg_repo.get_with(Options {
            names,
            stylesheet,
            halo: def.extra.halo,
            halo_opacity: svg_repo::halo_opacity_under_fade(fade),
            halo_color,
            use_extents: false,
        })?;

        let (x, y, w, he) = surface.ink_extents();

        let corner_x = point.x() - w / 2.0;

        let corner_y = point.y() - he / 2.0;

        'outer: for &(dx, dy) in OFFSETS.iter() {
            let corner_x = ctx.hint(corner_x + dx - 0.5) + 0.5;
            let corner_y = ctx.hint(corner_y + dy - 0.5) + 0.5;

            let bbox = Rect::new((corner_x, corner_y), (corner_x + w, corner_y + he));

            if collision.collides(&bbox) {
                continue;
            }

            let bbox_idx = collision.add(bbox);

            if def.min_text_zoom <= zoom {
                let name = row.get_string("name")?;

                let ele = extra.get("ele").and_then(Option::clone);

                // A spot height carries no name - its elevation is the whole label, so
                // an empty name is not on its own a reason to skip labelling.
                let has_ele =
                    def.extra.ele_only_label && ele.as_deref().is_some_and(|e| !e.is_empty());

                if !name.is_empty() || has_ele {
                    let name = replace(name, &def.extra.replacements);

                    to_label.push(PendingLabel {
                        point: Point::new(point.x() + dx, point.y() + dy),
                        icon_half_height: he / 2.0,
                        name: name.into_owned(),
                        ele,
                        bbox_idx,
                        alpha: fade,
                        color,
                        halo_color,
                        def,
                    });
                }
            }

            let _span = tracy_client::span!("features::paint_svg");

            context.set_source_surface(surface, corner_x - x, corner_y - y)?;

            context.paint_with_alpha(fade)?;

            break 'outer;
        }
    }

    Ok(to_label)
}

pub fn render_labels(
    _ctx: &Ctx,
    context: &Context,
    to_label: ToLabel,
    collision: &mut Collision,
) -> LayerRenderResult {
    let _span = tracy_client::span!("pois::render_labels");

    let defaults = TextOptions::default();

    for PendingLabel {
        point,
        icon_half_height: d,
        name,
        ele,
        bbox_idx,
        alpha,
        color,
        halo_color,
        def,
    } in to_label
    {
        let text_options = TextOptions {
            flo: FontAndLayoutOptions {
                style: if def.natural {
                    Style::Italic
                } else {
                    Style::Normal
                },
                size: def.extra.font_size,
                weight: def.extra.weight,
                ..Default::default()
            },
            // Labels take the icon's colour, so a POI reads as one unit.
            color: def.extra.text_color.unwrap_or(color),
            alpha,
            // Same red aura as the icon, so a restricted POI reads as one unit - but at
            // the icon's opacity, not the default: white is invisible at 0.75 and red is
            // a highlighter pen.
            halo_color: halo_color.unwrap_or(defaults.halo_color),
            halo_opacity: halo_color.map_or(defaults.halo_opacity, |_| {
                svg_repo::halo_opacity_under_fade(alpha)
            }),
            valign_by_placement: true,
            placements: &[
                (0.0, -d - 3.0),
                (0.0, d - 3.0),
                (0.0, -d - 5.0),
                (0.0, d - 1.0),
                (0.0, -d - 7.0),
                (0.0, d + 1.0),
            ],
            omit_bbox: Some(bbox_idx),
            sub_size_scale: Some(0.8),
            ..Default::default()
        };

        if def.with_ele
            && let Some(ele) = ele
        {
            draw_text(
                context,
                Some(collision),
                &point,
                format!("{name}\n{ele}").trim(),
                &text_options,
            )?
        } else {
            draw_text(context, Some(collision), &point, &name, &text_options)?
        };
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Category, POI_ENTRIES, POIS, build_poi_z_order_case, color_of, icon_key_of};
    use crate::render::colors;
    use std::collections::{HashMap, HashSet};

    /// Wider than any zoom the renderer is asked for, so every definition gets a chance.
    const ZOOMS: std::ops::RangeInclusive<u8> = 0..=24;

    /// `render_labels` builds an elevation-only label as `format!("{name}\n{ele}").trim()`,
    /// which needs `with_ele` to reach the elevation at all. Without it a nameless POI
    /// would claim a label slot and then draw an empty string.
    #[test]
    fn an_elevation_only_label_can_reach_its_elevation() {
        for (typ, defs) in POIS.iter() {
            for def in defs {
                assert!(
                    !def.extra.ele_only_label || def.with_ele,
                    "{typ} is labelled with its elevation alone but is not with_ele"
                );
            }
        }
    }

    #[test]
    fn no_definition_is_shadowed() {
        for (typ, defs) in POIS.iter() {
            let reachable: HashSet<usize> = ZOOMS
                .filter_map(|zoom| defs.iter().position(|def| def.is_active_at(zoom)))
                .collect();

            for idx in 0..defs.len() {
                assert!(
                    reachable.contains(&idx),
                    "definition {idx} of {typ} is never the first match for any zoom, \
                     so render_icons can never pick it - widen an earlier max_zoom or drop it"
                );
            }
        }
    }

    #[test]
    fn definitions_of_a_type_are_contiguous() {
        // A type's priority comes from its first entry, so its definitions have to sit
        // together; split apart, the table would read as if they ranked differently.
        let mut spans: HashMap<&str, (usize, usize, usize)> = HashMap::new();

        for (idx, (_, _, _, _, _, name, _)) in POI_ENTRIES.iter().enumerate() {
            spans
                .entry(name)
                .and_modify(|(_, last, count)| {
                    *last = idx;
                    *count += 1;
                })
                .or_insert((idx, idx, 1));
        }

        for (name, (first, last, count)) in spans {
            assert_eq!(
                last - first + 1,
                count,
                "definitions of {name} are split apart in POI_ENTRIES"
            );
        }
    }

    /// The legend groups POI types by the icon they draw and gives the whole group the
    /// category of whichever member ranks first, so two types sharing an icon have to agree
    /// on their category - otherwise one of them is quietly filed under the other's heading
    /// with nothing in the legend to show it happened.
    #[test]
    fn types_sharing_an_icon_share_a_category() {
        let mut by_icon: HashMap<&str, (&str, Category)> = HashMap::new();

        for (_, _, _, _, category, typ, extra) in POI_ENTRIES.iter() {
            let icon = icon_key_of(extra, typ);

            let (first_typ, first_category) = by_icon.entry(icon).or_insert((typ, *category));

            assert_eq!(
                first_category, category,
                "{typ} and {first_typ} both draw the {icon} icon but are in different \
                 categories, so the legend lists them under whichever ranks first"
            );
        }
    }

    #[test]
    fn every_definition_loads_its_icon() {
        use crate::render::svg_repo::{Options, SvgRepo};

        let mut repo = SvgRepo::new("images");
        let mut bad = vec![];

        for (_, _, _, _, _, typ, extra) in POI_ENTRIES.iter() {
            let key = icon_key_of(extra, typ);

            if repo
                .get_with(Options {
                    names: vec![key.to_string()],
                    halo: true,
                    ..Default::default()
                })
                .is_err()
            {
                bad.push(key.to_string());
            }
        }

        assert!(bad.is_empty(), "icons that failed to load: {bad:?}");
    }

    /// The tint overrides a `fill="…"` attribute but loses to an inline `style="fill:…"`,
    /// so an icon with colours of its own keeps them only in `style`. Black attributes are
    /// fine - those are what the tint replaces.
    #[test]
    fn an_icon_with_colours_of_its_own_states_them_in_style() {
        fn is_black(fill: &str) -> bool {
            let hex = fill.trim().trim_start_matches('#');

            let expanded = match hex.len() {
                3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
                6 => hex.to_owned(),
                _ => return false,
            };

            (0..3).all(|i| {
                u8::from_str_radix(&expanded[i * 2..i * 2 + 2], 16).is_ok_and(|v| v <= 0x14)
            })
        }

        /// The fill an element declares, by `style` (which the tint cannot override) or
        /// by attribute (which it can). Returns whether `style` was the source.
        fn declared_fill(el: &xmltree::Element) -> (Option<&str>, bool) {
            let styled = el.attributes.get("style").and_then(|s| {
                s.split(';')
                    .filter_map(|d| d.split_once(':'))
                    .find(|(k, _)| k.trim() == "fill")
                    .map(|(_, v)| v.trim())
            });

            styled.map_or_else(
                || (el.attributes.get("fill").map(String::as_str), false),
                |fill| (Some(fill), true),
            )
        }

        let mut bad = vec![];

        for (_, _, _, _, category, typ, extra) in POI_ENTRIES.iter() {
            let key = icon_key_of(extra, typ);

            // A missing file is `every_definition_loads_its_icon`'s to report, not this one.
            let Ok(svg) = std::fs::read_to_string(format!("images/{key}.svg")) else {
                continue;
            };

            let Ok(root) = xmltree::Element::parse(svg.as_bytes()) else {
                continue;
            };

            let tint = colors::rgb_hex(color_of(*category, extra));

            let mut walk = vec![(&root, None::<&str>)];

            while let Some((el, inherited)) = walk.pop() {
                let (own, own_styled) = declared_fill(el);
                let fill = own.or(inherited);

                for child in el.children.iter().filter_map(xmltree::XMLNode::as_element) {
                    walk.push((child, fill));
                }

                // The tint's selector only reaches <path>, and only a `style` on the path
                // itself outranks it - a fill inherited from an ancestor does not.
                if el.name.split(':').next_back() != Some("path") || own_styled {
                    continue;
                }

                let Some(fill) = fill else {
                    continue;
                };

                if fill.eq_ignore_ascii_case(&tint) || is_black(fill) {
                    continue;
                }

                bad.push(format!(
                    "{key}.svg draws a path in {fill}, which the {category:?} tint ({tint}) \
                     would repaint - state it as style=\"fill:{fill}\" on the path"
                ));
            }
        }

        assert!(
            bad.is_empty(),
            "icons losing their own colours:\n{}",
            bad.join("\n")
        );
    }
    #[test]
    fn types_sharing_an_icon_share_a_colour() {
        let mut by_icon: HashMap<&str, (&str, String)> = HashMap::new();

        for (_, _, _, _, category, typ, extra) in POI_ENTRIES.iter() {
            // Mirrors `legend::pois`, which gives `volcano` a legend entry of its own
            // rather than folding it into the `peak` icon's, and skips `*_noname`
            // entirely. Those are the types allowed to draw a shared icon in their own
            // colour, because the legend shows them separately.
            if *typ == "volcano" || typ.ends_with("_noname") {
                continue;
            }

            let icon = icon_key_of(extra, typ);

            let color = colors::rgb_hex(color_of(*category, extra));

            let (first_typ, first_color) =
                by_icon.entry(icon).or_insert_with(|| (typ, color.clone()));

            assert_eq!(
                *first_color, color,
                "{typ} draws the {icon} icon in {color} but {first_typ} draws it in \
                 {first_color} - the same glyph would appear on the map in two colours"
            );
        }
    }

    /// An icon drawn by the POI layer and by another layer must be asked for with the
    /// same tint in both places. The POI layer always supplies one, and the icon files
    /// carry no colour of their own, so a caller using the bare `get("name")` form draws
    /// that glyph in black - which is how obstacle markers along a line came out black
    /// beside red ones on a node.
    ///
    /// Scans the layer sources rather than listing the names, so a new `get("…")` is
    /// covered without anyone remembering to add it here.
    #[test]
    fn icons_shared_with_another_layer_are_tinted_there_too() {
        let poi_icons: HashSet<&str> = POI_ENTRIES
            .iter()
            .map(|(_, _, _, _, _, typ, extra)| icon_key_of(extra, typ))
            .collect();

        let mut bad = vec![];

        for entry in std::fs::read_dir("src/render/layers").expect("layers dir") {
            let path = entry.expect("dir entry").path();

            if path.extension().is_none_or(|e| e != "rs")
                || path.file_name().is_some_and(|n| n == "pois.rs")
            {
                continue;
            }

            let src = std::fs::read_to_string(&path).expect("read layer");

            // The convenience form, which passes no stylesheet.
            for (_, rest) in src
                .match_indices("svg_repo.get(\"")
                .map(|(i, m)| (i, &src[i + m.len()..]))
            {
                let Some(name) = rest.split('"').next() else {
                    continue;
                };

                if poi_icons.contains(name) {
                    bad.push(format!(
                        "{} asks for {name:?} with svg_repo.get(), which supplies no tint, \
                         but the POI layer draws the same icon tinted - pass the same \
                         colour here via get_with(Options {{ stylesheet, .. }})",
                        path.display()
                    ));
                }
            }
        }

        assert!(bad.is_empty(), "untinted shared icons:\n{}", bad.join("\n"));
    }

    #[test]
    fn the_z_order_case_escapes_apostrophes() {
        assert!(
            build_poi_z_order_case("type").contains("WHEN 'forester''s_lodge' THEN"),
            "an apostrophe in a type name must be doubled for SQL"
        );
    }
}
