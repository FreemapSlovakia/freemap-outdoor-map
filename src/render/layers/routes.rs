use crate::render::{
    Feature, Layers, RenderLayer,
    collision::Collision,
    ctx::Ctx,
    draw::{
        font_options::FontAndLayoutOptions,
        line_pattern::draw_line_pattern_scaled,
        offset_line::offset_line_string_parts,
        osmc_symbol::{osmc_symbol_image, osmc_symbol_shows},
        path_geom::{path_line_string_with_offset, walk_geometry_line_strings},
        text_on_line::{
            Align, Distribution, InlineImage, LabelPart, Repeat, TextOnLineOptions,
            draw_parts_on_line,
        },
    },
    layer_render_error::LayerRenderResult,
    projectable::TileProjectable,
    svg_repo::{Options, SvgRepo},
};
use cairo::{Context, RecordingSurface};
use colorsys::{Rgb, RgbRatio};
use geo::Geometry;
use std::{borrow::Cow, rc::Rc};

const COLOR_SQL: &str = r#"
  CASE
    WHEN "osmc:symbol" LIKE 'red:%' THEN 0
    WHEN "osmc:symbol" LIKE 'blue:%' THEN 1
    WHEN "osmc:symbol" LIKE 'green:%' THEN 2
    WHEN "osmc:symbol" LIKE 'yellow:%' THEN 3
    WHEN "osmc:symbol" LIKE 'black:%' THEN 4
    WHEN "osmc:symbol" LIKE 'white:%' THEN 5
    WHEN "osmc:symbol" LIKE 'orange:%' THEN 6
    WHEN "osmc:symbol" LIKE 'violet:%' THEN 7
    WHEN "osmc:symbol" LIKE 'purple:%' THEN 7
    WHEN colour = 'red' THEN 0
    WHEN colour = 'blue' THEN 1
    WHEN colour = 'green' THEN 2
    WHEN colour = 'yellow' THEN 3
    WHEN colour = 'black' THEN 4
    WHEN colour = 'white' THEN 5
    WHEN colour = 'orange' THEN 6
    WHEN colour = 'violet' THEN 7
    WHEN colour = 'purple' THEN 7
    ELSE 8
  END
"#;

/// A route's label item: name, ref and `osmc:symbol`, tab separated; null when all are missing.
/// Tabs and newlines in the values would split items, so they become spaces.
const LABEL_SQL: &str = r#"
  NULLIF(
    translate(COALESCE(name, ''), E'\t\n\r', '   ') || E'\t' ||
    translate(COALESCE(ref, ''), E'\t\n\r', '   ') || E'\t' ||
    translate(COALESCE("osmc:symbol", ''), E'\t\n\r', '   '),
    E'\t\t'
  )
"#;

const COLORS: [(&str, &str); 9] = [
    ("none", "a0a0a0"),
    ("purple", "c000c0"),
    ("orange", "ff8000"),
    ("white", "ffffff"),
    ("black", "000000"),
    ("yellow", "f0f000"),
    ("green", "00a000"),
    ("blue", "5050ff"),
    ("red", "ff3030"),
];

fn format_vec(vec: &[&str]) -> String {
    if vec.is_empty() {
        "'_x_'".to_string()
    } else {
        vec.iter()
            .map(|&item| format!("'{item}'"))
            .collect::<Vec<String>>()
            .join(",")
    }
}

fn get_routes_query(
    render: &Layers,
    include_networks: Option<Vec<&str>>,
    gen_suffix: &str,
) -> String {
    let mut lefts = Vec::<&str>::new();

    let mut rights = Vec::<&str>::new();

    let bool_hiking_kst = render.draws(RenderLayer::RoutesHikingKst);
    let bool_hiking = render.draws(RenderLayer::RoutesHiking) || bool_hiking_kst;

    if bool_hiking || bool_hiking_kst {
        lefts.extend_from_slice(&["hiking", "foot", "running"]);
    }

    if render.draws(RenderLayer::RoutesHorse) {
        lefts.push("horse");
    }

    if render.draws(RenderLayer::RoutesBicycle) {
        rights.extend_from_slice(&["bicycle", "mtb"]);
    }

    if render.draws(RenderLayer::RoutesSki) {
        rights.extend_from_slice(&["ski", "piste"]);
    }

    let lefts_in = format_vec(&lefts);

    let rights_in = format_vec(&rights);

    let mut conditions = Vec::new();

    if let Some(networks) = include_networks {
        let mut network_condition = String::from("network IN (");

        for (i, &network) in networks.iter().enumerate() {
            if i > 0 {
                network_condition.push(',');
            }
            network_condition.push('\'');
            network_condition.push_str(network);
            network_condition.push('\'');
        }

        network_condition.push(')');
        conditions.push(network_condition);
    }

    if bool_hiking_kst {
        conditions.push(String::from(r"operator ~* '\ykst\y|\ytanap\y'"));
    }

    let cond = if conditions.is_empty() {
        String::new()
    } else {
        format!("{} AND ", conditions.join(" AND "))
    };

    let bool_horse = render.draws(RenderLayer::RoutesHorse);
    let bool_bicycle = render.draws(RenderLayer::RoutesBicycle);
    let bool_ski = render.draws(RenderLayer::RoutesSki);

    format!("
        SELECT
            ST_Multi(ST_LineMerge(ST_Collect(geometry))) AS geometry,
            idx(arr1, 0) AS h_red,
            idx(arr1, 1) AS h_blue,
            idx(arr1, 2) AS h_green,
            idx(arr1, 3) AS h_yellow,
            idx(arr1, 4) AS h_black,
            idx(arr1, 5) AS h_white,
            idx(arr1, 6) AS h_orange,
            idx(arr1, 7) AS h_purple,
            idx(arr1, 8) AS h_none,
            idx(arr1, 10) AS h_red_loc,
            idx(arr1, 11) AS h_blue_loc,
            idx(arr1, 12) AS h_green_loc,
            idx(arr1, 13) AS h_yellow_loc,
            idx(arr1, 14) AS h_black_loc,
            idx(arr1, 15) AS h_white_loc,
            idx(arr1, 16) AS h_orange_loc,
            idx(arr1, 17) AS h_purple_loc,
            idx(arr1, 18) AS h_none_loc,
            idx(arr2, 20) AS b_red,
            idx(arr2, 21) AS b_blue,
            idx(arr2, 22) AS b_green,
            idx(arr2, 23) AS b_yellow,
            idx(arr2, 24) AS b_black,
            idx(arr2, 25) AS b_white,
            idx(arr2, 26) AS b_orange,
            idx(arr2, 27) AS b_purple,
            idx(arr2, 28) AS b_none,
            idx(arr2, 30) AS s_red,
            idx(arr2, 31) AS s_blue,
            idx(arr2, 32) AS s_green,
            idx(arr2, 33) AS s_yellow,
            idx(arr2, 34) AS s_black,
            idx(arr2, 35) AS s_white,
            idx(arr2, 36) AS s_orange,
            idx(arr2, 37) AS s_purple,
            idx(arr2, 38) AS s_none,
            idx(arr1, 40) AS r_red,
            idx(arr1, 41) AS r_blue,
            idx(arr1, 42) AS r_green,
            idx(arr1, 43) AS r_yellow,
            idx(arr1, 44) AS r_black,
            idx(arr1, 45) AS r_white,
            idx(arr1, 46) AS r_orange,
            idx(arr1, 47) AS r_purple,
            idx(arr1, 48) AS r_none,
            refs1,
            refs2,
            icount(arr1 - array[1000, 1010, 1020, 1030, 1040]) AS off1,
            icount(arr2 - array[1000, 1010, 1020, 1030, 1040]) AS off2
        FROM (
            SELECT
                array_to_string(
                    array(
                        SELECT itm
                        FROM unnest(
                            array_agg(
                                CASE WHEN
                                    osm_routes.type IN ({lefts_in})
                                THEN
                                    {LABEL_SQL}
                                ELSE
                                    null
                                END
                            )
                        ) AS itm GROUP BY itm ORDER BY COALESCE(NULLIF(split_part(itm, E'\\t', 1), ''), split_part(itm, E'\\t', 2)), itm
                    ),
                    E'\n'
                ) AS refs1,
                array_to_string(
                    array(
                        SELECT itm FROM unnest(
                            array_agg(
                                CASE
                                WHEN
                                    osm_routes.type IN ({rights_in})
                                THEN
                                    {LABEL_SQL}
                                ELSE
                                    null
                                END
                            )
                        ) AS itm GROUP BY itm ORDER BY COALESCE(NULLIF(split_part(itm, E'\\t', 1), ''), split_part(itm, E'\\t', 2)), itm
                    ),
                    E'\n'
                ) AS refs2,
                first(geometry) AS geometry,
                uniq(sort(array_agg(
                    CASE
                        WHEN osm_routes.type IN ({lefts_in}) THEN
                        CASE
                            WHEN {bool_horse} AND osm_routes.type = 'horse' THEN 40
                            WHEN
                                {bool_hiking} AND
                                osm_routes.type IN ('hiking', 'foot', 'running')
                            THEN (
                                CASE WHEN network IN ('iwn', 'nwn', 'rwn') THEN 0 ELSE 10 END
                            )
                            ELSE 1000
                        END +
                        {COLOR_SQL}
                        ELSE 1000
                    END
                ))) AS arr1,
                uniq(sort(array_agg(
                    CASE
                    WHEN osm_routes.type IN ({rights_in}) THEN
                    CASE
                        WHEN {bool_bicycle} AND osm_routes.type IN ('bicycle', 'mtb') THEN 20
                        WHEN {bool_ski} AND osm_routes.type IN ('ski', 'piste') THEN 30
                        ELSE 1000
                    END +
                        {COLOR_SQL}
                    ELSE
                        1000
                    END
                ))) AS arr2
            FROM
                osm_route_members{gen_suffix}
            JOIN
                osm_routes
            ON
                (osm_route_members{gen_suffix}.osm_id = osm_routes.osm_id AND state <> 'proposed')
            WHERE
                {cond}geometry && ST_Expand(ST_MakeEnvelope($1, $2, $3, $4, 3857), $5)
            GROUP BY
                member
        ) AS aaa
        GROUP BY
            h_red, h_blue, h_green, h_yellow, h_black, h_white, h_orange, h_purple, h_none,
            h_red_loc, h_blue_loc, h_green_loc, h_yellow_loc, h_black_loc, h_white_loc, h_orange_loc, h_purple_loc, h_none_loc,
            b_red, b_blue, b_green, b_yellow, b_black, b_white, b_orange, b_purple, b_none,
            s_red, s_blue, s_green, s_yellow, s_black, s_white, s_orange, s_purple, s_none,
            r_red, r_blue, r_green, r_yellow, r_black, r_white, r_orange, r_purple, r_none,
            off1, off2, refs1, refs2
    ")
}

pub async fn query_marking(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
    render: &Layers,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let zoom = ctx.zoom;

    let z = zoom
        + if render.draws(RenderLayer::RoutesHikingKst) {
            2
        } else {
            0
        };

    let sql = match z {
        9 => get_routes_query(render, Some(vec!["iwn", "icn"]), "_gen0"),
        10 => get_routes_query(render, Some(vec!["iwn", "nwn", "icn", "ncn"]), "_gen1"),
        11 => get_routes_query(
            render,
            Some(vec!["iwn", "nwn", "rwn", "icn", "ncn", "rcn"]),
            "_gen1",
        ),
        12.. => get_routes_query(render, None, ""),
        _ => return Ok(Vec::new()),
    };

    client
        .query(&sql, &ctx.bbox_query_params(Some(512.0)).as_params())
        .await
}

fn draw_offset_pattern(
    ctx: &Ctx,
    context: &Context,
    geom: &Geometry,
    offset: f64,
    wf: f64,
    sample: &RecordingSurface,
) -> cairo::Result<()> {
    walk_geometry_line_strings(geom, &mut |line| {
        for offset_part in offset_line_string_parts(line, offset) {
            draw_line_pattern_scaled(context, ctx.size, &offset_part, 0.5, wf / 2.0, sample)?;
        }

        Ok(())
    })
}

pub fn render_marking(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    to_render: Layers,
    svg_repo: &mut SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("routes::render_marking");

    let zoom = ctx.zoom;

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        let (zo, wf) = match zoom {
            ..=11 => (1.0, 1.5),
            12 => (2.0, 1.5),
            13.. => (3.0, 2.0),
        }; // offset from highway

        let df = 1.25;

        for color in &COLORS {
            if to_render.draws(RenderLayer::RoutesHorse) {
                let off = row.get_i32(&format!("r_{}", color.0))?;

                if off > 0 {
                    let offset = ((off as f64 - 1.0) * wf).mul_add(df, zo) + 0.5;

                    let sample = svg_repo.get_with(Options {
                        names: vec!["horse".into()],
                        stylesheet: Some(format!("path {{ fill: #{} }}", color.1)),
                        ..Default::default()
                    })?;

                    draw_offset_pattern(ctx, context, &geom, offset, wf, sample)?;
                }
            }

            if to_render.draws(RenderLayer::RoutesSki) {
                let off = row.get_i32(&format!("s_{}", color.0))?;

                if off > 0 {
                    let offset = -((off as f64 - 1.0) * wf).mul_add(2.0, zo) - 1.0;

                    let pattern = svg_repo.get_with(Options {
                        names: vec!["ski".into()],
                        stylesheet: Some(format!("path {{ fill: #{} }}", color.1)),
                        ..Default::default()
                    })?;

                    draw_offset_pattern(ctx, context, &geom, offset, wf, pattern)?;
                }
            }

            if to_render.draws(RenderLayer::RoutesBicycle) {
                let off = row.get_i32(&format!("b_{}", color.0))?;

                if off > 0 {
                    let offset = -((off as f64 - 1.0) * wf).mul_add(2.0, zo) - 1.0;

                    context.save()?;

                    walk_geometry_line_strings(&geom, &mut |part| {
                        path_line_string_with_offset(context, part, offset);

                        cairo::Result::Ok(())
                    })?;

                    context.set_line_width(wf * 2.0);
                    context.set_line_join(cairo::LineJoin::Round);
                    context.set_line_cap(cairo::LineCap::Round);

                    let rgb: RgbRatio = Rgb::from_hex_str(color.1).expect("color").as_ratio();
                    context.set_source_rgb(rgb.r(), rgb.g(), rgb.b());
                    context.set_dash(&[0.001, wf * 3.0], 0.0);

                    context.stroke()?;

                    context.restore()?;
                }
            }

            if to_render.draws(RenderLayer::RoutesHiking)
                || to_render.draws(RenderLayer::RoutesHikingKst)
            {
                {
                    let off = row.get_i32(&format!("h_{}", color.0))?;

                    if off > 0 {
                        let offset = ((off as f64 - 1.0) * wf).mul_add(df, zo) + 0.5;

                        context.save()?;

                        walk_geometry_line_strings(&geom, &mut |part| {
                            path_line_string_with_offset(context, part, offset);

                            cairo::Result::Ok(())
                        })?;

                        context.set_line_width(wf);
                        context.set_line_join(cairo::LineJoin::Round);
                        context.set_line_cap(cairo::LineCap::Butt);
                        let rgb: RgbRatio = Rgb::from_hex_str(color.1).expect("color").as_ratio();
                        context.set_source_rgb(rgb.r(), rgb.g(), rgb.b());
                        context.set_dash(&[], 0.0);

                        context.stroke()?;

                        context.restore()?;
                    }
                }

                {
                    let off = row.get_i32(&format!("h_{}_loc", color.0))?;

                    if off > 0 {
                        let offset = ((off as f64 - 1.0) * wf).mul_add(df, zo) + 0.5;

                        context.save()?;

                        walk_geometry_line_strings(&geom, &mut |part| {
                            path_line_string_with_offset(context, part, offset);

                            cairo::Result::Ok(())
                        })?;

                        context.set_line_width(wf);
                        context.set_line_join(cairo::LineJoin::Round);
                        context.set_line_cap(cairo::LineCap::Butt);
                        let rgb: RgbRatio = Rgb::from_hex_str(color.1).expect("color").as_ratio();
                        context.set_source_rgb(rgb.r(), rgb.g(), rgb.b());
                        context.set_dash(&[wf * 3.0, wf], 0.0);

                        context.stroke()?;

                        context.restore()?;
                    }
                }
            }
        }
    }

    Ok(())
}

pub async fn query_labels(
    ctx: &Ctx,
    client: &tokio_postgres::Client,
    render: &Layers,
) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
    let query = get_routes_query(render, None, "");

    client
        .query(&query, &ctx.bbox_query_params(Some(2048.0)).as_params())
        .await
}

/// How much of each route a label shows; shorter ones are tried where a longer one does not fit.
#[derive(Clone, Copy, PartialEq)]
enum Detail {
    /// `Name (ref)`.
    Full,
    /// The ref only.
    Ref,
    /// No text.
    Symbols,
}

/// A route of a label: its name, its ref unless the symbol shows it, and its symbol.
struct Item<'a> {
    name: &'a str,
    r#ref: &'a str,
    image: Option<Rc<InlineImage>>,
}

/// Items of `refs`: name, ref and `osmc:symbol` separated by tabs, one per line. The legend's
/// items are bare names.
fn label_items<'a>(refs: &'a str, svg_repo: &SvgRepo) -> Vec<Item<'a>> {
    refs.split('\n')
        .filter(|item| !item.is_empty())
        .map(|item| {
            let mut fields = item.split('\t');
            let name = fields.next().unwrap_or_default();
            let r#ref = fields.next().unwrap_or_default();
            let symbol = fields.next().unwrap_or_default();
            let image = osmc_symbol_image(symbol, svg_repo);

            // A ref the symbol already shows, as E8 in `:blue::E8:yellow`, would be said twice;
            // a longer one stays, as the symbol shrinks it past reading.
            let shown =
                image.is_some() && r#ref.chars().count() <= 3 && osmc_symbol_shows(symbol, r#ref);

            Item {
                name,
                r#ref: if shown { "" } else { r#ref },
                image,
            }
        })
        .collect()
}

/// Label parts of `items` at `detail`. Symbols of routes sharing a text go before it together.
fn label_parts<'a>(items: &[Item<'a>], detail: Detail) -> Vec<LabelPart<'a>> {
    let mut groups: Vec<(Cow<str>, Vec<Rc<InlineImage>>)> = Vec::new();

    for item in items {
        let (name, r#ref) = (item.name, item.r#ref);

        let text = match detail {
            Detail::Full if !name.is_empty() && !r#ref.is_empty() => {
                Cow::Owned(format!("{name} ({ref})"))
            }
            Detail::Full if name.is_empty() => Cow::Borrowed(r#ref),
            Detail::Full => Cow::Borrowed(name),
            Detail::Ref => Cow::Borrowed(r#ref),
            Detail::Symbols => Cow::Borrowed(""),
        };

        // Sorted by name, which a shorter text need not follow.
        match groups
            .iter_mut()
            .find(|(other, _)| !text.is_empty() && *other == text)
        {
            Some((_, images)) => images.extend(item.image.clone()),
            None if item.image.is_some() || !text.is_empty() => {
                groups.push((text, item.image.clone().into_iter().collect()));
            }
            None => {}
        }
    }

    let mut parts = Vec::new();
    let mut text = String::new();

    // Symbols alone need no dot between them.
    let separator = if groups.iter().all(|(text, _)| text.is_empty()) {
        " "
    } else {
        " · "
    };

    for (i, (name, images)) in groups.into_iter().enumerate() {
        if i > 0 {
            text.push_str(separator);
        }

        if !images.is_empty() {
            if !text.is_empty() {
                parts.push(LabelPart::Text(Cow::Owned(std::mem::take(&mut text))));
            }

            parts.extend(images.into_iter().map(LabelPart::Image));

            if !name.is_empty() {
                text.push(' ');
            }
        }

        text.push_str(&name);
    }

    if !text.is_empty() {
        parts.push(LabelPart::Text(Cow::Owned(text)));
    }

    parts
}

/// The label of `refs` at each detail, longest first, without empty or repeated ones.
fn label_variants<'a>(refs: &'a str, svg_repo: &SvgRepo) -> Vec<Vec<LabelPart<'a>>> {
    let items = label_items(refs, svg_repo);
    let mut variants: Vec<Vec<LabelPart>> = Vec::new();

    for detail in [Detail::Full, Detail::Ref, Detail::Symbols] {
        let parts = label_parts(&items, detail);

        if !parts.is_empty() && variants.last().is_none_or(|last| !same_parts(last, &parts)) {
            variants.push(parts);
        }
    }

    variants
}

/// Whether two labels read the same; the images of one row's variants are the same `Rc`s.
fn same_parts(a: &[LabelPart], b: &[LabelPart]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (LabelPart::Text(a), LabelPart::Text(b)) => a == b,
            (LabelPart::Image(a), LabelPart::Image(b)) => Rc::ptr_eq(a, b),
            _ => false,
        })
}

pub fn render_labels(
    ctx: &Ctx,
    context: &Context,
    rows: Vec<Feature>,
    collision: &mut Collision,
    svg_repo: &SvgRepo,
) -> LayerRenderResult {
    let _span = tracy_client::span!("routes::render_labels");

    for row in rows {
        let geom = row.get_geometry()?.project_to_tile(&ctx.tile_projector);

        let variants1 = label_variants(row.get_string("refs1")?, svg_repo);
        let variants1: Vec<&[LabelPart]> = variants1.iter().map(Vec::as_slice).collect();
        let off1 = row.get_i32("off1")?;

        let variants2 = label_variants(row.get_string("refs2")?, svg_repo);
        let variants2: Vec<&[LabelPart]> = variants2.iter().map(Vec::as_slice).collect();
        let off2 = row.get_i32("off2")?;

        walk_geometry_line_strings(&geom, &mut |geom| {
            let mut options = TextOnLineOptions {
                flo: FontAndLayoutOptions {
                    size: 11.0,
                    ..Default::default()
                },
                halo_opacity: 0.2,
                distribution: Distribution::Align {
                    align: Align::Center,
                    repeat: Repeat::Spaced(500.0),
                },
                keep_offset_side: true,
                ..Default::default()
            };

            for (variants, offset) in [
                (&variants1, -f64::from(off1).mul_add(2.5, 9.0)),
                (&variants2, f64::from(off2).mul_add(2.5, 10.0)),
            ] {
                if variants.is_empty() {
                    continue;
                }

                options.offset = offset;

                let _drawn =
                    draw_parts_on_line(context, geom, variants, Some(collision), &options)?;
            }

            cairo::Result::Ok(())
        })?;
    }

    Ok(())
}
