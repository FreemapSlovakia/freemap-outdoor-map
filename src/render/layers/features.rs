use super::feature_z_order::build_feature_z_order_case;
use crate::render::{
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
    svg_repo::Options,
    svg_repo::SvgRepo,
};
use core::f64;
use geo::{Point, Rect};
use pangocairo::pango::{AttrList, AttrSize, SCALE, Style, Weight};
use postgres::Client;
use std::borrow::Cow;
use std::{collections::HashMap, sync::LazyLock};

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

struct Def {
    min_zoom: u8,
    min_text_zoom: u8,
    with_ele: bool,
    natural: bool,
    extra: Extra<'static>,
}

#[rustfmt::skip]
static POIS: LazyLock<HashMap<&'static str, Vec<Def>>> = LazyLock::new(|| {
    const Y: bool = true;
    const N: bool = false;
    const NN: u8 = u8::MAX;

    let spring_replacements = build_replacements(&[
        (r"\b[Mm]inerálny\b", "min."),
        (r"\b[Pp]rameň\b", "prm."),
        (r"\b[Ss]tud(ničk|ň)a\b", "stud."),
        (r"\b[Vv]yvieračka\b", "vyv."),
    ]);

    let church_replacements = build_replacements(&[
        (r"^[Kk]ostol\b *", ""),
        (r"\b([Ss]vät\w+|Sv\.)", "sv."),
    ]);

    let chapel_replacements = build_replacements(&[
        (r"^[Kk]aplnka\b *", ""),
        (r"\b([Ss]vät\w+|Sv\.)", "sv."),
    ]);

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

    let entries = vec![
        (12, 12, N, N, "aerodrome", Extra {
            replacements: build_replacements(&[(r"^[Ll]etisko\b *", "")]),
            ..Extra::default()
        }),
        // (12, 12, Y, N, "guidepost", Extra { icon: Some("guidepost_x"), weight: Weight::Bold, max_zoom: 12, ..Extra::default() }),
        (13, 13, Y, N, "guidepost", Extra { icon: Some("guidepost_xx"), weight: Weight::Bold, max_zoom: 13, ..Extra::default() }),
        (14, 14, Y, N, "guidepost", Extra { icon: Some("guidepost_xx"), weight: Weight::Bold, ..Extra::default() }),
        (10, 10, Y, Y, "peak1", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (11, 11, Y, Y, "peak2", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (12, 12, Y, Y, "peak3", Extra { icon: Some("peak"), font_size: 13.0, halo: false, ..Extra::default() }),
        (13, 13, Y, Y, "peak", Extra { font_size: 13.0, halo: false, ..Extra::default() }),
        (14, 14, N, N, "castle", Extra {
            replacements: build_replacements(&[(r"^[Hh]rad\b *", "")]),
            ..Extra::default()
        }),
        (14, 15, Y, Y, "arch", Extra::default()),
        (14, 15, Y, Y, "cave_entrance", Extra {
            replacements: build_replacements(&[
                (r"^[Jj]jaskyňa\b *", ""),
                (r"\b[Jj]jaskyňa$", "j."),
                (r"\b[Pp]riepasť\b", "p."),
            ]),
            ..Extra::default()
        }),
        (14, 15, Y, Y, "spring", Extra { replacements: spring_replacements.clone(), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (14, 15, Y, Y, "waterfall", Extra {
            replacements: build_replacements(&[
                (r"^[Vv]odopád\b *", ""),
                (r"\b[Vv]odopád$", "vdp."),
            ]),
            text_color: colors::WATER_LABEL,
            ..Extra::default()
        }),
        (14, 15, N, N, "drinking_water", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (14, 15, N, N, "water_point", Extra { text_color: colors::WATER_LABEL, icon: Some("drinking_water"), ..Extra::default() }),
        (14, 15, N, N, "water_well", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (14, 15, Y, N, "monument", Extra::default()),
        (14, 15, Y, Y, "viewpoint", Extra {
            replacements: build_replacements(&[
                (r"^[Vv]yhliadka\b *", ""),
                (r"\b[Vv]yhliadka$", "vyhl."),
            ]),
            ..Extra::default()
        }),
        (14, 15, Y, N, "mine", Extra { icon: Some("mine"), ..Extra::default() }),
        (14, 15, Y, N, "adit", Extra { icon: Some("mine"), ..Extra::default() }),
        (14, 15, Y, N, "mineshaft", Extra { icon: Some("mine"), ..Extra::default() }),
        (14, 15, Y, N, "disused_mine", Extra::default()),
        (14, 15, Y, N, "hotel", Extra {
            replacements: build_replacements(&[(r"^[Hh]otel\b *", "")]),
            ..Extra::default()
        }),
        (14, 15, Y, N, "chalet", Extra {
            replacements: build_replacements(&[
                (r"^[Cc]hata\b *", ""),
                (r"\b[Cc]hata$", "ch."),
            ]),
            ..Extra::default()
        }),
        (14, 15, Y, N, "hostel", Extra::default()),
        (14, 15, Y, N, "motel", Extra {
            replacements: build_replacements(&[(r"^[Mm]otel\b *", "")]),
            ..Extra::default()
        }),
        (14, 15, Y, N, "guest_house", Extra::default()),
        (14, 15, Y, N, "apartment", Extra::default()),
        (14, 15, Y, N, "wilderness_hut", Extra::default()),
        (14, 15, Y, N, "alpine_hut", Extra::default()),
        (14, 15, Y, N, "camp_site", Extra::default()),
        (14, 15, N, N, "attraction", Extra::default()),
        (14, 15, N, N, "hospital", Extra {
            replacements: build_replacements(&[(r"^[Nn]emocnica\b", "Nem.")]),
            ..Extra::default()
        }),
        (14, NN, N, N, "townhall", Extra {
            replacements: chapel_replacements.clone(),
            ..Extra::default()
        }),
        (14, 15, N, N, "chapel", Extra::default()),
        (14, 15, N, N, "church", Extra {
            replacements: church_replacements.clone(),
            ..Extra::default()
        }),
        (14, 15, N, N, "cathedral", Extra {
            replacements: church_replacements.clone(),
            icon: Some("church"),
            ..Extra::default()
        }),
        (14, 15, N, N, "synagogue", Extra::default()),
        (14, 15, N, N, "mosque", Extra::default()),
        (14, 15, Y, N, "tower_observation", Extra::default()),
        (14, 15, Y, N, "archaeological_site", Extra::default()),
        (14, 15, N, N, "station", Extra::default()),
        (14, 15, N, N, "halt", Extra { icon: Some("station"), ..Extra::default() }),
        (14, 15, N, N, "bus_station", Extra::default()),
        (14, 15, N, N, "water_park", Extra::default()),
        (14, 15, N, N, "museum", Extra::default()),
        (14, 15, N, N, "manor", Extra::default()),
        (14, 15, N, N, "free_flying", Extra::default()),
        (14, 15, N, N, "forester's_lodge", Extra::default()),
        (14, 15, N, N, "horse_riding", Extra::default()),
        (14, 15, N, N, "equestrian", Extra { icon: Some("horse_riding"), ..Extra::default() }),
        (14, 15, N, N, "horse_racing", Extra { icon: Some("horse_riding"), ..Extra::default() }), // TODO use different icon
        (14, 15, N, N, "skiing", Extra::default()),
        (14, 15, N, N, "golf_course", Extra::default()),
        // TODO (14, 14, N, N, "recycling", Extra { text_color: colors::AREA_LABEL, ..Extra::default() }), // { icon: null } // has no icon yet - render as area name
        (15, NN, Y, N, "guidepost_noname", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (15, 15, Y, Y, "saddle", Extra { font_size: 13.0, halo: false, ..Extra::default() }),
        (15, 15, Y, Y, "mountain_pass", Extra { icon: Some("saddle"), font_size: 13.0, halo: false, ..Extra::default() }),
        (15, 16, N, N, "ruins", Extra::default()),
        (15, 16, N, N, "generator_wind", Extra::default()),
        (15, 16, N, N, "chimney", Extra::default()),
        (15, 16, N, N, "fire_station", Extra {
            replacements: build_replacements(&[(r"^([Hh]asičská zbrojnica|[Pp]ožiarná stanica)\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "community_centre", Extra {
            replacements: build_replacements(&[(r"\b[Cc]entrum voľného času\b", "CVČ")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "police", Extra {
            replacements: build_replacements(&[(r"^[Pp]olícia\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "office", Extra::default()),           // information=office
        (15, 16, N, N, "hunting_stand", Extra::default()),
        (15, 16, Y, N, "shelter", Extra::default()),
        // (15, 16, Y, N, 'shopping_cart', Extra::default()),
        (15, 16, Y, N, "lean_to", Extra::default()),
        (15, 16, Y, N, "public_transport", Extra::default()),
        (15, 16, Y, N, "picnic_shelter", Extra::default()),
        (15, 16, Y, N, "basic_hut", Extra::default()),
        (15, 16, Y, N, "weather_shelter", Extra::default()),
        (15, 16, N, N, "pharmacy", Extra {
            replacements: build_replacements(&[(r"^[Ll]ekáreň\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "cinema", Extra {
            replacements: build_replacements(&[(r"^[Kk]ino\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "theatre", Extra {
            replacements: build_replacements(&[(r"^[Dd]ivadlo\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "memorial", Extra {
            replacements: build_replacements(&[(r"^[Pp]amätník\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "pub", Extra::default()),
        (15, 16, N, N, "cafe", Extra {
            replacements: build_replacements(&[(r"^[Kk]aviareň\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "bar", Extra::default()),
        (15, 16, N, N, "restaurant", Extra {
            replacements: build_replacements(&[(r"^[Rr]eštaurácia\b *", "")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "convenience", Extra::default()),
        (15, 16, N, N, "greengrocer", Extra::default()),
        (15, 16, N, N, "farm", Extra { icon: Some("greengrocer"), ..Extra::default()}),
        (15, 16, N, N, "supermarket", Extra::default()),
        (15, 16, N, N, "fast_food", Extra::default()),
        (15, 16, N, N, "confectionery", Extra::default()),
        (15, 16, N, N, "pastry", Extra { icon: Some("confectionery"), ..Extra::default() }),
        (15, 16, N, N, "fuel", Extra::default()),
        (15, 16, N, N, "post_office", Extra::default()),
        (15, 16, N, N, "bunker", Extra::default()),
        (15, NN, N, N, "mast_other", Extra::default()),
        (15, NN, N, N, "tower_other", Extra::default()),
        (15, NN, N, N, "tower_communication", Extra::default()),
        (15, NN, N, N, "mast_communication", Extra { icon: Some("tower_communication"), ..Extra::default() }),
        (15, 16, N, N, "tower_bell_tower", Extra::default()),
        (15, 16, N, N, "water_tower", Extra::default()),
        (15, 16, N, N, "bus_stop", Extra::default()),
        (15, 16, N, N, "sauna", Extra::default()),
        (15, 16, N, N, "taxi", Extra::default()),
        (15, 16, N, N, "bicycle", Extra::default()),
        (15, 15, N, Y, "tree_protected", Extra { text_color: colors::TREE, ..Extra::default() }),
        (15, 15, N, Y, "tree", Extra::default()),
        (15, 16, N, N, "bird_hide", Extra::default()),
        (15, 16, N, N, "dam", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (15, 16, N, N, "school", Extra { replacements: school_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, "college", Extra { replacements: college_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, "university", Extra { replacements: university_replacements.clone(), ..Extra::default() }),
        (15, 16, N, N, "kindergarten", Extra {
            replacements: build_replacements(&[(r"[Mm]atersk(á|ou) [Šš]k[oô]lk?(a|ou)", "MŠ")]),
            ..Extra::default()
        }),
        (15, 16, N, N, "climbing", Extra::default()),
        (15, 16, N, N, "shooting", Extra::default()),
        (16, 17, N, Y, "rock", Extra::default()),
        (16, 17, N, Y, "stone", Extra::default()),
        (16, 17, N, Y, "sinkhole", Extra::default()),
        (16, 17, N, N, "building", Extra::default()),
        (16, 17, N, N, "weir", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, "miniature_golf", Extra::default()),
        (16, 17, N, N, "soccer", Extra::default()),
        (16, 17, N, N, "tennis", Extra::default()),
        (16, 17, N, N, "basketball", Extra::default()),
        (16, 17, N, N, "volleyball", Extra::default()),
        (16, 17, N, N, "running", Extra::default()),
        (16, 17, N, N, "athletics", Extra { icon: Some("running"), ..Extra::default() }),
        (16, 17, N, N, "swimming", Extra { icon: Some("water_park"), ..Extra::default() }),
        (16, 17, N, N, "cycling", Extra::default()),
        (16, 17, N, N, "ice_skating", Extra::default()),
        (16, NN, Y, N, "guidepost_noname", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (16, NN, Y, N, "route_marker", Extra { icon: Some("guidepost_x"), ..Extra::default() }),
        (16, NN, N, N, "picnic_table", Extra::default()),
        (16, NN, N, N, "outdoor_seating", Extra::default()),
        (16, 17, N, N, "picnic_site", Extra::default()),
        (16, 16, N, N, "board", Extra::default()),
        (16, 17, N, N, "map", Extra::default()),
        (16, 17, N, N, "artwork", Extra::default()),
        (16, 17, N, N, "fountain", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, NN, N, N, "watering_place", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, NN, N, N, "feeding_place", Extra { icon: Some("manger"), ..Extra::default() }),
        (16, NN, N, N, "game_feeding", Extra { icon: Some("manger"), ..Extra::default() }),
        (16, 17, N, N, "playground", Extra {
            replacements: build_replacements(&[(r"^[Dd]etské ihrisko\b", "")]),
            ..Extra::default()
        }),
        (16, 17, N, N, "water_works", Extra { text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, "reservoir_covered", Extra { icon: Some("water_works"), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, "pumping_station", Extra { icon: Some("water_works"), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, "wastewater_plant", Extra { icon: Some("water_works"), text_color: colors::WATER_LABEL, ..Extra::default() }),
        (16, 17, N, N, "cross", Extra::default()),
        (17, 18, N, N, "boundary_stone", Extra::default()),
        (17, 18, N, N, "marker", Extra { icon: Some("boundary_stone"), ..Extra::default() }),
        (17, 18, N, N, "wayside_shrine", Extra::default()),
        (17, 18, N, N, "cross", Extra::default()), // NOTE cross is also on lower zoom
        (17, 18, N, N, "wayside_cross", Extra { icon: Some("cross"), ..Extra::default() }), // NOTE cross is also on lower zoom
        (17, 18, N, N, "tree_shrine", Extra { icon: Some("cross"), ..Extra::default() }), // NOTE cross is also on lower zoom
        (17, NN, N, N, "firepit", Extra::default()),
        (17, NN, N, N, "toilets", Extra::default()),
        (17, NN, N, N, "bench", Extra::default()),
        (17, 18, N, N, "beehive", Extra::default()),
        (17, 18, N, N, "apiary", Extra { icon: Some("beehive"), ..Extra::default() }),
        (17, NN, N, N, "lift_gate", Extra::default()),
        (17, NN, N, N, "swing_gate", Extra { icon: Some("lift_gate"), ..Extra::default() }),
        (17, NN, N, N, "ford", Extra::default()),
        (17, 19, N, N, "parking", Extra { font_size: 10.0, text_color: colors::AREA_LABEL, ..Extra::default() }), // { font: { haloOpacity: 0.5 } },
        (18, 19, N, N, "building_ruins", Extra { icon: Some("ruins"), ..Extra::default() }),
        (18, 19, N, N, "post_box", Extra::default()),
        (18, 19, N, N, "telephone", Extra::default()),
        (18, NN, N, N, "gate", Extra::default()),
        (18, NN, N, N, "waste_disposal", Extra::default()),
        (19, NN, N, N, "waste_basket", Extra::default()),
        ];

    let mut pois = HashMap::new();

    for (min_zoom, min_text_zoom, with_ele, natural, name, extra) in entries.into_iter() {
        pois
            .entry(name)
            .or_insert_with(Vec::new)
            .push(Def {
                min_zoom,
                min_text_zoom,
                with_ele,
                natural,
                extra,
            });
    }

    pois
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
) -> LayerRenderResult {
    let _span = tracy_client::span!("features::render");

    let zoom = ctx.zoom;

    let rows = ctx.legend_features("features", || {
        let mut selects = vec![];

        selects.push(
            "SELECT
                osm_id,
                geometry,
                name AS n,
                hstore(ARRAY['ele', tags->'ele', 'isolation', tags->'isolation']) AS h,
                CASE WHEN isolation > 4500 THEN 'peak1'
                    WHEN isolation BETWEEN 3000 AND 4500 THEN 'peak2'
                    WHEN isolation BETWEEN 1500 AND 3000 THEN 'peak3'
                    ELSE 'peak'
                END AS type
            FROM
                osm_features
            NATURAL LEFT JOIN
                isolations
            WHERE
                geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                type = 'peak' AND name <> ''
            ",
        );

        if zoom >= 13 {
            selects.push(
                "SELECT
                    osm_id,
                    geometry,
                    name AS n,
                    hstore('ele', tags->'ele') AS h,
                    CASE WHEN type = 'guidepost' AND name = '' THEN 'guidepost_noname' ELSE type END
                FROM
                    osm_features
                WHERE
                    type = 'guidepost' AND
                    geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
                ",
            );
        }

        if (12..=13).contains(&zoom) {
            selects.push(
                "SELECT
                    osm_id,
                    geometry,
                    name AS n,
                    hstore('ele', tags->'ele') AS h,
                    type
                FROM
                    osm_features
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
                    COALESCE(NULLIF(name, ''), tags->'ref', '') AS n,
                    hstore(ARRAY[
                        'ele', tags->'ele',
                        'access', tags->'access',
                        'hot', (type = 'hot_spring')::text,
                        'drinkable', tags->'drinking_water',
                        'refitted', tags->'refitted',
                        'intermittent', COALESCE(tags->'intermittent', tags->'seasonal'),
                        'water_characteristic', tags->'water_characteristic'
                    ]) AS h,
                    CASE
                        WHEN
                            type = 'guidepost' AND
                            name = ''
                        THEN 'guidepost_noname'
                        WHEN
                            type = 'tree' AND
                            tags->'protected' <> 'no'
                        THEN 'tree_protected'
                        WHEN type = 'communications_tower'
                        THEN 'tower_communication'
                        WHEN
                            type = 'shelter' AND
                            tags->'shelter_type' IN (
                                'shopping_cart', 'lean_to', 'public_transport', 'picnic_shelter',
                                'basic_hut', 'weather_shelter'
                            )
                        THEN tags->'shelter_type'
                        WHEN
                            type IN ('mine', 'adit', 'mineshaft') AND
                            tags->'disused' <> 'no'
                        THEN 'disused_mine'
                        WHEN type IN ('hot_spring', 'geyser', 'spring_box')
                        THEN 'spring'
                        WHEN type IN ('tower', 'mast')
                        THEN
                            type || '_' || CASE tags->'tower:type'
                                WHEN 'communication' THEN 'communication'
                                WHEN 'observation' THEN 'observation'
                                WHEN 'bell_tower' THEN 'bell_tower'
                                ELSE 'other'
                            END
                        ELSE type
                    END AS type
                FROM
                    osm_features
                WHERE
                    geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                    (
                        type <> 'saddle' OR
                        NOT EXISTS (
                            SELECT 1
                            FROM osm_features b
                            WHERE
                                type = 'mountain_pass' AND
                                osm_features.osm_id = b.osm_id
                        )
                    ) AND
                    (
                        type <> 'tree' OR
                        tags->'protected' NOT IN ('', 'no') OR
                        tags->'denotation' = 'natural_monument'
                    ) AND
                    (
                        type NOT IN ('saddle', 'mountain_pass') OR
                        name <> ''
                    )
                    {w}
            ");

            selects.push(&z14_sql);

            selects.push("
                SELECT
                    osm_id,
                    geometry,
                    name AS n,
                    hstore('') as h,
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
                    name AS n,
                    hstore('') AS h,
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
                    name AS n,
                    hstore('') AS h,
                    type
                FROM
                    osm_shops
                WHERE
                    geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                    type IN (
                        'convenience', 'fuel', 'confectionery', 'pastry', 'bicycle', 'supermarket', 'greengrocer', 'farm'
                    )
            ");

            selects.push("
                SELECT
                    osm_id,
                    ST_LineInterpolatePoint(geometry, 0.5) AS geometry,
                    name AS n,
                    hstore('') AS h,
                    type
                FROM
                    osm_feature_lines
                WHERE
                    geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5) AND
                    type IN ('dam', 'weir', 'ford')
            ");
        }

        let z_order_case = build_feature_z_order_case("type");

        let sql = format!(r"
            SELECT
                *
            FROM
                ({}) AS tmp
            ORDER BY
                {z_order_case},
                h->'isolation' DESC NULLS LAST,
                CASE
                    WHEN (h->'ele') ~ '^\s*-?\d+(\.\d+)?\s*$' THEN (h->'ele')::real
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

            let h = row.get_hstore("h")?;

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

                    let is_mineral = h
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
                        && h.get("refitted")
                            .is_some_and(|r| r.as_deref() == Some("yes"))
                    {
                        key.push_str("|refitted");
                        names.push("refitted_spring".into());
                    }

                    let fill = if h.get("hot").is_some_and(|r| r.as_deref() == Some("true")) {
                        key.push_str("|hot");

                        "#e11919"
                    } else {
                        "#0064ff"
                    };

                    if h.get("intermittent")
                        .is_some_and(|r| r.as_deref() == Some("yes"))
                    {
                        key.push_str("|tmp");
                        names.push("intermittent".into());
                    }

                    stylesheet.push_str(&format!("#spring {{ fill: {fill} }}"));

                    match h.get("drinkable").and_then(Option::as_deref) {
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
                    let name = row.get_string("n")?;

                    if !name.is_empty() {
                        let name = replace(name, &def.extra.replacements);

                        to_label.push((
                            Point::new(point.x() + dx, point.y() + dy),
                            he / 2.0,
                            name.into_owned(),
                            h.get("ele").and_then(Option::clone),
                            bbox_idx,
                            def,
                        ));
                    }
                }

                let _span = tracy_client::span!("features::paint_svg");

                context.set_source_surface(surface, corner_x - x, corner_y - y)?;

                context.paint_with_alpha(
                    if typ != "cave_entrance"
                        && h.get("access").is_some_and(|access| {
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
                placements: &[-d - 3.0, d - 3.0, -d - 5.0, d - 1.0, -d - 7.0, d + 1.0],
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

            if !drawn {
                continue;
            }
        }
    }

    Ok(())
}
