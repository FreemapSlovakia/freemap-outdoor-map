use crate::render::{
    collision::Collision,
    colors::{self, Color, ContextExt},
    draw::create_pango_layout::{FontAndLayoutOptions, create_layout_checked},
};
use cairo::Context;
use geo::{Point, Rect};
use pangocairo::{
    functions::layout_path,
    pango::{AttrList, SCALE},
};

#[derive(Copy, Clone)]
pub struct TextOptions<'a> {
    pub alpha: f64,
    pub color: Color,
    pub halo_color: Color,
    pub halo_opacity: f64,
    pub halo_width: f64,
    pub placements: &'a [(f64, f64)],
    pub flo: FontAndLayoutOptions,
    pub valign_by_placement: bool,
    pub omit_bbox: Option<usize>,
}

impl Default for TextOptions<'_> {
    fn default() -> Self {
        TextOptions {
            alpha: 1.0,
            color: colors::BLACK,
            halo_color: colors::WHITE,
            halo_opacity: 0.75,
            halo_width: 1.5,
            flo: Default::default(),
            placements: &[
                (0.0, 0.0),
                (0.0, 3.0),
                (0.0, -3.0),
                (0.0, 6.0),
                (0.0, -6.0),
                (0.0, 9.0),
                (0.0, -9.0),
            ],
            valign_by_placement: false,
            omit_bbox: None,
        }
    }
}

pub fn draw_text(
    context: &Context,
    collision: Option<&mut Collision>,
    point: &Point,
    text: &str,
    options: &TextOptions,
) -> cairo::Result<Option<usize>> {
    draw_text_with_attrs(context, collision, point, text, None, options)
}

pub fn draw_text_with_attrs(
    context: &Context,
    collision: Option<&mut Collision>,
    point: &Point,
    text: &str,
    attrs: Option<AttrList>,
    options: &TextOptions,
) -> cairo::Result<Option<usize>> {
    if text.is_empty() {
        return Ok(Some(0));
    }

    let TextOptions {
        alpha,
        color,
        halo_color,
        halo_opacity,
        halo_width,
        placements,
        flo,
        valign_by_placement,
        omit_bbox,
    } = options;

    let layout = create_layout_checked(
        context,
        "text",
        text,
        attrs,
        flo,
        Some((point.x(), point.y())),
    )?;

    let mut m: Option<(f64, f64)> = None;

    let (ext, _) = layout.extents();

    let layout_x = ext.x() as f64 / SCALE as f64;

    let layout_y = ext.y() as f64 / SCALE as f64;

    let layout_width = ext.width() as f64 / SCALE as f64;

    let layout_height = ext.height() as f64 / SCALE as f64;

    let x = point.x() - (layout_x + layout_width / 2.0);

    let mut cap_height: Option<f64> = None;
    let mut first_baseline: Option<f64> = None;
    let mut last_baseline: Option<f64> = None;

    let mut i: usize = 0;

    'outer: for &(dx, dy) in *placements {
        i += 1;

        let y = if *valign_by_placement {
            let ch = *cap_height.get_or_insert_with(|| {
                layout
                    .font_description()
                    .map(|desc| {
                        let ctx = layout.context();
                        let metrics = ctx.metrics(Some(&desc), None);
                        metrics.ascent() as f64 / SCALE as f64
                    })
                    .unwrap_or(layout_height + layout_y)
            });

            if first_baseline.is_none() || last_baseline.is_none() {
                let mut li = layout.iter();
                let first = li.baseline() as f64 / SCALE as f64;
                let mut last = first;
                while li.next_line() {
                    last = li.baseline() as f64 / SCALE as f64;
                }
                first_baseline = Some(first);
                last_baseline = Some(last);
            }

            let fb = first_baseline.expect("first_baseline");
            let lb = last_baseline.expect("last_baseline");

            if dy > 0.0 {
                fb + ch
            } else if dy < 0.0 {
                lb
            } else {
                layout_y + layout_height / 2.0
            }
        } else {
            layout_y + layout_height / 2.0
        };

        let y = dy + point.y() - y;

        let x = dx + x;

        let mut items = Vec::new();

        let mut li = layout.iter();

        loop {
            let (ext, _) = li.line_extents();

            let line_x = ext.x() as f64 / SCALE as f64;

            let line_y = ext.y() as f64 / SCALE as f64;

            let line_width = ext.width() as f64 / SCALE as f64;

            let line_height = ext.height() as f64 / SCALE as f64;

            let ci = Rect::new(
                (x - halo_width + line_x, y - halo_width + line_y),
                (
                    x + halo_width + line_x + line_width,
                    y + halo_width + line_y + line_height,
                ),
            );

            if let Some(ref collision) = collision {
                if let Some(omit_idx) = *omit_bbox {
                    if collision.collides_with_exclusion(&ci, omit_idx) {
                        continue 'outer;
                    }
                } else if collision.collides(&ci) {
                    continue 'outer;
                }
            }

            items.push(ci);

            if !li.next_line() {
                break;
            }
        }

        if let Some(collision) = collision {
            for item in items {
                let _ = collision.add(item);
            }
        }

        m = Some((x, y));

        break;
    }

    let (x, y) = match m {
        Some(a) => a,
        None => return Ok(None),
    };

    context.push_group();

    context.move_to(x, y);

    layout_path(context, &layout);

    context.status()?;

    context.set_source_color_a(*halo_color, *halo_opacity);
    context.set_dash(&[], 0.0);
    context.set_line_join(cairo::LineJoin::Round);
    context.set_line_width(halo_width * 2.0);
    context.stroke_preserve()?;
    context.set_source_color(*color);

    context.fill()?;

    context.pop_group_to_source()?;

    context.paint_with_alpha(*alpha)?;

    Ok(Some(i))
}
