use super::poi_z_order::build_poi_z_order_case;
use crate::render::{
    categories::Category,
    collision::Collision,
    colors::{self, Color},
    ctx::Ctx,
    draw::{
        create_pango_layout::FontAndLayoutOptions,
        text::{TextOptions, draw_text, draw_text_with_attrs},
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    regex_replacer::{Replacement, build_replacements, replace},
    svg_repo::{Options, SvgRepo},
};
use core::f64;
use geo::{Point, Rect};
use pangocairo::pango::{AttrList, AttrSize, SCALE, Style, Weight};
use postgres::Client;
use std::borrow::Cow;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

struct Extra<'a> {
    replacements: Vec<Replacement<'a>>,
    icon: Option<&'a str>,
    font_size: f64,
    weight: Weight,
    text_color: Color,
    max_zoom: u8,
    stylesheet: Option<&'a str>,
    halo: bool,
}

impl Default for Extra<'_> {
    fn default() -> Self {
        Self {
            replacements: vec![],
            icon: None,
            font_size: 12.0,
            weight: Weight::Normal,
            text_color: colors::BLACK,
            max_zoom: u8::MAX,
            stylesheet: None,
            halo: true,
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
    pub(crate) fn is_active_at(&self, zoom: u8) -> bool {
        self.min_zoom <= zoom && self.extra.max_zoom >= zoom
    }

    pub(crate) fn icon_key<'a>(&'a self, typ: &'a str) -> &'a str {
        self.extra.icon.unwrap_or(typ)
    }
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

    use Category::*;

    #[rustfmt::skip]
    let entries = vec![
        (12, 12, N, N, Poi, "aerodrome", Extra {
            replacements: build_replacements(&[(r"^[Ll]etisko\b *", "")]),
            ..Extra::default()
        }),
        // (12, 12, Y, N, "guidepost", Extra { icon: Some("guidepost_x"), weight: Weight::Bold, max_zoom: 12, ..Extra::default() }),
        (13, 13, Y, N, Poi, "guidepost", Extra { icon: Some("guidepost_xx"), weight: Weight::Bold, max_zoom: 13, ..Extra::default() }),
        (14, 14, Y, N, Poi, "guidepost", Extra { icon: Some("guidepost_xx"), weight: Weight::Bold, ..Extra::default() }),
        (10, 10, Y, Y, NaturalPoi, "peak1", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (11, 11, Y, Y, NaturalPoi, "peak2", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (12, 12, Y, Y, NaturalPoi, "peak3", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (13, 13, Y, Y, NaturalPoi, "peak", Extra { font_size: 13.0, halo: false, ..Extra::default() }),
        (14, 14, N, N, Poi, "castle", Extra {
            replacements: build_replacements(&[(r"^[Hh]rad\b *", "")]),
            ..Extra::default()
        }),
        (14, 15, Y, Y, NaturalPoi, "arch", Extra::default()),
        (14, 15, Y, Y, NaturalPoi, "cave_entrance", Extra {
            replacements: build_replacements(&[
                (r"^[Jj]jaskyňa\b *", ""),
                (r"\b[Jj]jaskyňa$", "j."),
                (r"\b[Pp]riepasť\b", "p."),
            ]),
            ..Extra::default()
        }),
        (14, 15, Y, Y, Water, "spring", Extra { replacements: spring_replacements.clone(), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (14, 15, Y, Y, Water, "waterfall", Extra {
            replacements: build_replacements(&[
                (r"^[Vv]odopád\b *", ""),
                (r"\b[Vv]odopád$", "vdp."),
            ]),
            text_color: colors::WATER_LABEL,
            ..Extra::default()
        }),
        (14, 15, N, N, Water, "drinking_water", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (14, 15, N, N, Water, "water_point", Extra { text_color: colors::WATER_LABEL, icon: Some("drinking_water"), ..Extra::default() }),
        (14, 15, N, N, Water, "water_well", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (14, 15, Y, N, Poi, "monument", Extra::default()),
        (14, 15, Y, Y, Poi, "viewpoint", Extra {
            replacements: build_replacements(&[
                (r"^[Vv]yhliadka\b *", ""),
                (r"\b[Vv]yhliadka$", "vyhl."),
            ]),
            ..Extra::default()
        }),
        (14, 15, Y, N, Poi, "historic_mine", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (14, 15, Y, N, Poi, "adit", Extra { icon: Some("mine"), ..Extra::default() }),
        (14, 15, Y, N, Poi, "mineshaft", Extra { icon: Some("mine"), ..Extra::default() }),
        (14, 15, Y, N, Poi, "disused_adit", Extra { icon: Some("disused_mine"), ..Extra::default() }),
        (14, 15, Y, N, Poi, "disused_mineshaft", Extra { icon: Some("disused_mine"), ..Extra::default() }),
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
        (14, 15, Y, N, Accommodation, "apartment", Extra::default()),
        (14, 15, Y, N, Accommodation, "wilderness_hut", Extra::default()),
        (14, 15, Y, N, Accommodation, "alpine_hut", Extra::default()),
        (14, 15, Y, N, Accommodation, "camp_site", Extra::default()),
        (14, 15, N, N, Poi, "attraction", Extra::default()),
        (14, 15, N, N, Institution, "hospital", Extra {
            replacements: build_replacements(&[(r"^[Nn]emocnica\b", "Nem.")]),
            ..Extra::default()
        }),
        (14, 15, N, N, Institution, "townhall", Extra {
            replacements: chapel_replacements.clone(),
            ..Extra::default()
        }),
        (14, 15, N, N, Institution, "chapel", Extra::default()),
        (14, 15, N, N, Institution, "church", Extra {
            replacements: church_replacements.clone(),
            ..Extra::default()
        }),
        (14, 15, N, N, Institution, "cathedral", Extra {
            replacements: church_replacements.clone(),
            icon: Some("church"),
            ..Extra::default()
        }),
        (14, 15, N, N, Institution, "synagogue", Extra::default()),
        (14, 15, N, N, Institution, "mosque", Extra::default()),
        (14, 15, Y, N, Poi, "tower_observation", Extra::default()),
        (14, 15, Y, N, Poi, "archaeological_site", Extra::default()),
        (14, 15, N, N, Railway, "station", Extra::default()),
        (14, 15, N, N, Railway, "halt", Extra { icon: Some("station"), ..Extra::default() }),
        (14, 15, N, N, Poi, "bus_station", Extra::default()),
        (14, 15, N, N, Poi, "water_park", Extra::default()),
        (14, 15, N, N, Institution, "museum", Extra::default()),
        (14, 15, N, N, Institution, "manor", Extra::default()),
        (14, 15, N, N, Sport, "free_flying", Extra::default()),
        (14, 15, N, N, Poi, "forester's_lodge", Extra::default()),
        (14, 15, N, N, Sport, "horse_riding", Extra::default()),
        (16, 17, N, N, Sport, "leisure_horse_riding", Extra { icon: Some("horse_riding"), ..Extra::default() }),
        (14, 15, N, N, Sport, "equestrian", Extra { icon: Some("horse_riding"), ..Extra::default() }),
        (14, 15, N, N, Sport, "horse_racing", Extra { icon: Some("horse_riding"), ..Extra::default() }), // TODO use different icon
        (14, 15, N, N, Sport, "skiing", Extra::default()),
        (14, 15, N, N, Poi, "golf_course", Extra::default()),
        // TODO (14, 14, N, N, "recycling", Extra { text_color: colors::AREA_LABEL, ..Extra::default() }), // { icon: null } // has no icon yet - render as area name
        (15, NN, Y, N, Poi, "guidepost_noname", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (15, 15, Y, Y, NaturalPoi, "saddle", Extra { font_size: 13.0, halo: false, ..Extra::default() }),
        (15, 15, Y, Y, NaturalPoi, "mountain_pass", Extra { icon: Some("saddle"), font_size: 13.0, halo: false, ..Extra::default() }),
        (15, 16, N, N, Poi, "ruins", Extra::default()),
        (15, 16, N, N, Poi, "generator_wind", Extra::default()),
        (15, 16, N, N, Poi, "chimney", Extra::default()),
        (15, 16, N, N, Institution, "fire_station", Extra {
            replacements: build_replacements(&[(r"^([Hh]asičská zbrojnica|[Pp]ožiarná stanica)\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "community_centre", Extra {
            replacements: build_replacements(&[(r"\b[Cc]entrum voľného času\b", "CVČ")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "police", Extra {
            replacements: build_replacements(&[(r"^[Pp]olícia\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "office", Extra::default()),           // information=office
        (15, 16, N, N, Accommodation, "hunting_stand", Extra::default()),
        (15, 16, Y, N, Accommodation, "shelter", Extra::default()),
        (15, 16, Y, N, Accommodation, "lean_to", Extra::default()),
        (15, 16, Y, N, Accommodation, "public_transport", Extra::default()),
        (15, 16, Y, N, Accommodation, "picnic_shelter", Extra::default()),
        (15, 16, Y, N, Accommodation, "basic_hut", Extra::default()),
        (15, 16, Y, N, Accommodation, "weather_shelter", Extra::default()),
        (15, 16, N, N, Institution, "pharmacy", Extra {
            replacements: build_replacements(&[(r"^[Ll]ekáreň\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "cinema", Extra {
            replacements: build_replacements(&[(r"^[Kk]ino\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Institution, "theatre", Extra {
            replacements: build_replacements(&[(r"^[Dd]ivadlo\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Poi, "memorial", Extra {
            replacements: build_replacements(&[(r"^[Pp]amätník\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, GastroPoi, "pub", Extra::default()),
        (15, 16, N, N, GastroPoi, "cafe", Extra {
            replacements: build_replacements(&[(r"^[Kk]aviareň\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, GastroPoi, "bar", Extra::default()),
        (15, 16, N, N, GastroPoi, "restaurant", Extra {
            replacements: build_replacements(&[(r"^[Rr]eštaurácia\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, GastroPoi, "convenience", Extra::default()),
        (15, 16, N, N, GastroPoi, "greengrocer", Extra::default()),
        (15, 16, N, N, GastroPoi, "farm", Extra { icon: Some("greengrocer"), ..Extra::default()}),
        (15, 16, N, N, GastroPoi, "supermarket", Extra::default()),
        (15, 16, N, N, GastroPoi, "fast_food", Extra::default()),
        (15, 16, N, N, GastroPoi, "confectionery", Extra::default()),
        (15, 16, N, N, GastroPoi, "pastry", Extra { icon: Some("confectionery"), ..Extra::default() }),
        (15, 16, N, N, Poi, "fuel", Extra::default()),
        (15, 16, N, N, Institution, "post_office", Extra::default()),
        (15, 16, N, N, Poi, "bunker", Extra::default()),
        (15, 16, N, N, Poi, "historic_bunker", Extra { icon: Some("bunker"), ..Extra::default() }),
        (15, NN, N, N, Poi, "mast", Extra::default()),
        (15, NN, N, N, Poi, "tower", Extra::default()),
        (15, NN, N, N, Poi, "tower_communication", Extra::default()),
        (15, NN, N, N, Poi, "communications_tower", Extra { icon: Some("tower_communication"), ..Extra::default() }),
        (15, NN, N, N, Poi, "mast_communication", Extra { icon: Some("tower_communication"), ..Extra::default() }),
        (15, 16, N, N, Poi, "tower_bell_tower", Extra::default()),
        (15, 16, N, N, Poi, "water_tower", Extra::default()),
        (15, 16, N, N, Poi, "bus_stop", Extra::default()),
        (15, 16, N, N, Poi, "sauna", Extra::default()),
        (15, 16, N, N, Poi, "taxi", Extra::default()),
        (15, 16, N, N, Poi, "bicycle", Extra::default()),
        (15, 15, N, Y, NaturalPoi, "tree_protected", Extra { text_color: colors::TREE, ..Extra::default() }),
        (15, 15, N, Y, NaturalPoi, "tree", Extra::default()),
        (15, 16, N, N, Poi, "bird_hide", Extra::default()),
        (15, 16, N, N, Water, "dam", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (15, 16, N, N, Institution, "school", Extra { replacements: school_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, Institution, "college", Extra { replacements: college_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, Institution, "university", Extra { replacements: university_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, Institution, "kindergarten", Extra {
            replacements: build_replacements(&[(r"[Mm]atersk(á|ou) [Šš]k[oô]lk?(a|ou)", "MŠ")]),
            ..Extra::default()
        }),
        (15, 16, N, N, Sport, "climbing", Extra::default()),
        (15, 16, N, N, Sport, "shooting", Extra::default()),
        (16, 17, N, Y, NaturalPoi, "rock", Extra::default()),
        (16, 17, N, Y, NaturalPoi, "stone", Extra::default()),
        (16, 17, N, Y, NaturalPoi, "sinkhole", Extra::default()),
        (16, 17, N, N, Other, "building", Extra::default()),
        (16, 17, N, N, Water, "weir", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, Sport, "miniature_golf", Extra::default()),
        (16, 17, N, N, Sport, "leisure_miniature_golf", Extra { icon: Some("miniature_golf"), ..Extra::default() }),
        (16, 17, N, N, Sport, "soccer", Extra::default()),
        (16, 17, N, N, Sport, "tennis", Extra::default()),
        (16, 17, N, N, Sport, "basketball", Extra::default()),
        (16, 17, N, N, Sport, "volleyball", Extra::default()),
        (16, 17, N, N, Sport, "running", Extra::default()),
        (16, 17, N, N, Sport, "athletics", Extra { icon: Some("running"), ..Extra::default() }),
        (16, 17, N, N, Sport, "swimming", Extra { icon: Some("water_park"), ..Extra::default() }),
        (16, 17, N, N, Sport, "cycling", Extra::default()),
        (16, 17, N, N, Sport, "ice_skating", Extra::default()),
        (16, NN, Y, N, Poi, "guidepost_noname", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (16, NN, Y, N, Poi, "route_marker", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (16, NN, N, N, Poi, "picnic_table", Extra::default()),
        (16, NN, N, N, Poi, "outdoor_seating", Extra::default()),
        (16, 17, N, N, Poi, "picnic_site", Extra::default()),
        (16, 16, N, N, Poi, "board", Extra::default()),
        (16, 17, N, N, Poi, "map", Extra::default()),
        (16, 17, N, N, Poi, "artwork", Extra::default()),
        (16, 17, N, N, Water, "fountain", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, NN, N, N, Water, "watering_place", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, NN, N, N, Poi, "feeding_place", Extra { icon: Some("manger"), ..Extra::default() }),
        (16, NN, N, N, Poi, "game_feeding", Extra { icon: Some("manger"), ..Extra::default() }),
        (16, 17, N, N, Poi, "playground", Extra {
            replacements: build_replacements(&[(r"^[Dd]etské ihrisko\b", "")]),
            ..Extra::default()
        }),
        (16, 17, N, N, Water, "water_works", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, Water, "reservoir_covered", Extra { icon: Some("water_works"), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, Water, "pumping_station", Extra { icon: Some("water_works"), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, Water, "wastewater_plant", Extra { icon: Some("water_works"), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, Poi, "cross", Extra::default()),
        (17, 18, N, N, Poi, "boundary_stone", Extra::default()),
        (17, 18, N, N, Poi, "marker", Extra { icon: Some("boundary_stone"), ..Extra::default() }),
        (17, 18, N, N, Poi, "wayside_shrine", Extra::default()),
        (17, 18, N, N, Poi, "cross", Extra::default()), // NOTE cross is also on lower zoom
        (17, 18, N, N, Poi, "wayside_cross", Extra { icon: Some("cross"), ..Extra::default() }), // NOTE cross is also on lower zoom
        (17, 18, N, N, Water, "tree_shrine", Extra { icon: Some("cross"), ..Extra::default() }), // NOTE cross is also on lower zoom
        (17, NN, N, N, Poi, "firepit", Extra::default()),
        (17, NN, N, N, Poi, "toilets", Extra::default()),
        (17, NN, N, N, Poi, "bench", Extra::default()),
        (17, 18, N, N, Poi, "beehive", Extra::default()),
        (17, 18, N, N, Poi, "apiary", Extra { icon: Some("beehive"), ..Extra::default() }),
        (17, NN, N, N, Poi, "lift_gate", Extra::default()),
        (17, NN, N, N, Poi, "swing_gate", Extra { icon: Some("lift_gate"), ..Extra::default() }),
        (17, NN, N, N, Water, "ford", Extra::default()),
        (17, 19, N, N, Poi, "parking", Extra { font_size: 10.0, text_color: colors::AREA_LABEL, ..Extra::default() }), // { font: { haloOpacity: 0.5 } },
        (18, 19, N, N, Other, "building_ruins", Extra { icon: Some("ruins"), ..Extra::default() }),
        (18, 19, N, N, Poi, "post_box", Extra::default()),
        (18, 19, N, N, Poi, "telephone", Extra::default()),
        (18, NN, N, N, Poi, "gate", Extra::default()),
        (18, NN, N, N, Poi, "waste_disposal", Extra::default()),
        (19, NN, N, N, Poi, "waste_basket", Extra::default()),
        ];

    entries
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
            extra: Extra {
                replacements: extra.replacements.clone(),
                icon: extra.icon,
                font_size: extra.font_size,
                weight: extra.weight,
                text_color: extra.text_color,
                max_zoom: extra.max_zoom,
                stylesheet: extra.stylesheet,
                halo: extra.halo,
            },
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

    for &r in RADII.iter() {
        for pos in 0..8 {
            offsets[idx] = offset_at(r, pos);
            idx += 1;
        }
    }

    offsets
});

pub fn render(
    ctx: &Ctx,
    client: &mut Client,
    collision: &mut Collision,
    svg_repo: &mut SvgRepo,
    kst_only: bool,
) -> LayerRenderResult {
    let _span = tracy_client::span!("pois::render");

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("pois", || {
        let mut selects = vec![];

        // TODO add hiking-only
        let kst_cond = if kst_only { r"AND (type <> 'guidepost' OR tags->'operator' ~* '\ykst\y|\ytanap\y')"} else { "" };

        selects.push(
            "SELECT
                osm_id,
                geometry,
                name,
                hstore(ARRAY['ele', tags->'ele', 'isolation', tags->'isolation']) AS extra,
                CASE WHEN isolation > 4500 THEN 'peak1'
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
                type = 'peak' AND
                name <> ''
            ",
        );

        let gte_z13_sql;

        if zoom >= 13 {
            gte_z13_sql = format!("SELECT
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
                ");

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
            let mut omit_types = vec!["'peak'".to_string()];

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

        z14_sql = format!("
            SELECT
                osm_id,
                geometry,
                COALESCE(NULLIF(name, ''), tags->'ref', '') AS name,
                hstore(ARRAY[
                    'ele', tags->'ele',
                    'access', tags->'access',
                    'hot', (type = 'hot_spring')::text,
                    'drinkable', tags->'drinking_water',
                    'refitted', tags->'refitted',
                    'intermittent', COALESCE(tags->'intermittent', tags->'seasonal'),
                    'water_characteristic', tags->'water_characteristic'
                ]) AS extra,
                CASE
                    WHEN
                        type = 'guidepost' AND
                        name = ''
                    THEN 'guidepost_noname'
                    WHEN
                        type = 'tree' AND
                        tags->'protected' <> 'no'
                    THEN 'tree_protected'
                    WHEN
                        type = 'shelter' AND
                        tags->'shelter_type' IN (
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
                    WHEN type IN ('tower', 'mast')
                    THEN
                        type || CASE tags->'tower:type'
                            WHEN 'communication' THEN '_communication'
                            WHEN 'observation' THEN '_observation'
                            WHEN 'bell_tower' THEN '_bell_tower'
                            ELSE ''
                        END
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
                ) AND
                (
                    type NOT IN ('saddle', 'mountain_pass') OR
                    COALESCE(NULLIF(name, ''), tags->'ref', '') <> ''
                )
                {w} {kst_cond}
            ");

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

            selects.push("
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
            ");
        }

        if zoom >= 15 {
            selects.push("
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
            ");

            selects.push("
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
                    type IN (
                        'convenience', 'confectionery', 'pastry', 'bicycle', 'supermarket', 'greengrocer', 'farm'
                    )
            ");

            selects.push("
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
            ");
        }

        let z_order_case = build_poi_z_order_case("type");

        let sql = format!(r"
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

        let _span = tracy_client::span!("features::query");

        client.query(&sql, &ctx.bbox_query_params(Some(1024.0)).as_params())
    })?;

    let mut to_label = Vec::<(Point, f64, String, Option<String>, usize, &Def)>::new();

    let context = ctx.context;

    {
        let _span = tracy_client::span!("features::paint_svgs");

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

            let key = def.extra.icon.unwrap_or(typ);

            let (key, names, stylesheet) = match key {
                "spring" => {
                    let mut stylesheet = String::new();

                    let is_mineral = extra
                        .get("water_characteristic")
                        .is_some_and(|v| v.is_some() && v.as_deref() != Some(""));

                    let mut key = (if is_mineral {
                        "mineral-spring"
                    } else {
                        "spring"
                    })
                    .to_string();

                    let mut names = vec![key.clone()];

                    if !is_mineral
                        && extra
                            .get("refitted")
                            .is_some_and(|r| r.as_deref() == Some("yes"))
                    {
                        key.push_str("|refitted");
                        names.push("refitted_spring".into());
                    }

                    let fill = if extra
                        .get("hot")
                        .is_some_and(|r| r.as_deref() == Some("true"))
                    {
                        key.push_str("|hot");

                        "#e11919"
                    } else {
                        "#0064ff"
                    };

                    if extra
                        .get("intermittent")
                        .is_some_and(|r| r.as_deref() == Some("yes"))
                    {
                        key.push_str("|tmp");
                        names.push("intermittent".into());
                    }

                    stylesheet.push_str(&format!("#spring {{ fill: {fill} }}"));

                    match extra.get("drinkable").and_then(Option::as_deref) {
                        Some("yes" | "treated") => {
                            key.push_str("|drinkable");
                            names.push("drinkable_spring".into());
                            stylesheet.push_str(r#"#drinkable { fill: #00ff00 } "#);
                        }
                        Some("no") => {
                            key.push_str("|not_drinkable");
                            names.push("drinkable_spring".into());
                            stylesheet.push_str(r#"#drinkable { fill: #ff0000 } "#);
                        }
                        _ => {}
                    }

                    (Cow::Owned(key), names, Some(stylesheet))
                }
                _ => (
                    Cow::Borrowed(key),
                    vec![key.to_string()],
                    def.extra.stylesheet.map(str::to_string),
                ),
            };

            let surface = svg_repo.get_extra(
                &key,
                Some({
                    || Options {
                        names,
                        stylesheet,
                        halo: def.extra.halo,
                        use_extents: false,
                    }
                }),
            )?;

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

                    if !name.is_empty() {
                        let name = replace(name, &def.extra.replacements);

                        to_label.push((
                            Point::new(point.x() + dx, point.y() + dy),
                            he / 2.0,
                            name.into_owned(),
                            extra.get("ele").and_then(Option::clone),
                            bbox_idx,
                            def,
                        ));
                    }
                }

                let _span = tracy_client::span!("features::paint_svg");

                context.set_source_surface(surface, corner_x - x, corner_y - y)?;

                context.paint_with_alpha(
                    if typ != "cave_entrance"
                        && extra.get("access").is_some_and(|access| {
                            matches!(access.as_deref(), Some("private" | "no"))
                        })
                    {
                        0.33
                    } else {
                        1.0
                    },
                )?;

                break 'outer;
            }
        }
    }

    {
        let _span = tracy_client::span!("features::labels");

        for (point, d, name, ele, bbox_idx, def) in to_label.into_iter() {
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
                color: def.extra.text_color,
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
                ..Default::default()
            };

            let drawn = if def.with_ele
                && let Some(ele) = ele
            {
                let attr_list = AttrList::new();

                let mut scale_attr =
                    AttrSize::new((text_options.flo.size * 0.8 * SCALE as f64) as i32);
                scale_attr.set_start_index(name.len() as u32 + 1);

                attr_list.insert(scale_attr);

                draw_text_with_attrs(
                    context,
                    Some(collision),
                    &point,
                    format!("{}\n{}", name, ele).trim(),
                    Some(attr_list),
                    &text_options,
                )?
            } else {
                draw_text(context, Some(collision), &point, &name, &text_options)?
            };

            if drawn.is_none() {
                continue;
            }
        }
    }

    Ok(())
}
