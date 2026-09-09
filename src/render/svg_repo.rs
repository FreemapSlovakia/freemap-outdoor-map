use cairo::{Content, RecordingSurface, Rectangle};
use gio::glib::{self};
use rsvg::LoadingError;
use std::{collections::HashMap, fs::read_to_string, path::PathBuf};
use xmltree::{Element, EmitterConfig, XMLNode};

pub struct SvgRepo {
    base: PathBuf,
    svg_map: HashMap<String, RecordingSurface>,
}

#[derive(Debug, thiserror::Error)]
#[error("{msg}{}", source.as_deref().map_or_else(String::new, |err| format!(": {err}")))]
pub struct SvgRepoError {
    msg: String,
    source: Option<Box<dyn std::error::Error + Sync + Send>>,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub names: Vec<String>,
    pub stylesheet: Option<String>,
    pub halo: bool,
    pub use_extents: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            names: vec![],
            stylesheet: None,
            halo: false,
            use_extents: true,
        }
    }
}

impl From<&str> for Options {
    fn from(value: &str) -> Self {
        Self {
            names: vec![value.into()],
            ..Default::default()
        }
    }
}

/// Width of the white glow stroked under a haloed icon, in the icon's own user units.
const HALO_WIDTH: f64 = 3.0;

/// How far the glow reaches outside the icon outline: half the stroke, since a stroke
/// straddles the path it follows.
const HALO_PAD: f64 = HALO_WIDTH / 2.0;

/// The glow, painted under the icon. `opacity_prop` is `stroke-opacity` when the style
/// goes on the icon's only element - there the stroke has to be faded on its own, or it
/// would fade the icon with it - and `opacity` when it goes on a `<use>` that repaints
/// the whole icon behind itself, where the copy as a whole is what has to fade.
///
/// `round` caps and joins are what make [`pad_viewport`] exact: with them the glow is
/// the icon outline offset by `HALO_PAD` in every direction, so the rendered ink starts
/// exactly `HALO_PAD` before the icon does. Miter joins would spike further at sharp
/// corners, and butt caps would stop short at the end of an open subpath; either way the
/// ink would sit off-centre against the icon and `render_icons`, which positions by ink,
/// would land the icon off the pixel grid and blur it.
fn halo_style(opacity_prop: &str) -> String {
    format!(
        "stroke:#fff;stroke-width:{HALO_WIDTH};{opacity_prop}:0.5;\
         stroke-linecap:round;stroke-linejoin:round;paint-order:stroke"
    )
}

/// Grows the document viewport by [`HALO_PAD`] on every side.
///
/// librsvg clips to the viewport, so without this an icon whose outline reaches its own
/// canvas edge would have the glow sliced off there - and, worse, the surviving ink
/// would no longer sit symmetrically around the icon, which is what `render_icons` uses
/// to place it on the pixel grid. Growing the canvas here means an icon file needs no
/// hand-added padding of its own: it can be drawn tight to its edges.
///
/// Icons that already carry padding are unchanged. The added room stays empty, and the
/// callers measure ink, not canvas.
fn pad_viewport(svg: &mut Element) {
    let parse = |v: &str| v.trim().trim_end_matches("px").trim().parse::<f64>().ok();

    let width = svg.attributes.get("width").and_then(|v| parse(v));
    let height = svg.attributes.get("height").and_then(|v| parse(v));

    let view_box = svg.attributes.get("viewBox").and_then(|vb| {
        let nums = vb
            .split([' ', ',', '\t', '\n'])
            .filter(|s| !s.is_empty())
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;

        <[f64; 4]>::try_from(nums).ok()
    });

    // No viewBox means user units are px, so width/height are the viewport. With
    // neither there is nothing to grow from - leave the document as it is rather than
    // inventing a size that would rescale the icon.
    let [vx, vy, vw, vh] = match (view_box, width, height) {
        (Some(vb), _, _) => vb,
        (None, Some(w), Some(h)) => [0.0, 0.0, w, h],
        _ => return,
    };

    if vw <= 0.0 || vh <= 0.0 {
        return;
    }

    svg.attributes.insert(
        "viewBox".into(),
        format!(
            "{} {} {} {}",
            vx - HALO_PAD,
            vy - HALO_PAD,
            vw + 2.0 * HALO_PAD,
            vh + 2.0 * HALO_PAD
        ),
    );

    // width/height are px and may scale the viewBox, so the padding is converted into px
    // at that same scale - anything else resizes the icon instead of the canvas. When
    // they are absent they default to 100%, which leaves the document with no intrinsic
    // size for `get_extra` to render into (it falls back to 16x16); writing them from
    // the viewBox keeps the icon at 1:1 and gives the renderer a real size. Growing the
    // viewBox without them would shrink the icon by (vw + 2 * HALO_PAD) / vw.
    let (w, sx) = width.map_or((vw, 1.0), |w| (w, w / vw));
    let (h, sy) = height.map_or((vh, 1.0), |h| (h, h / vh));

    svg.attributes
        .insert("width".into(), (w + 2.0 * HALO_PAD * sx).to_string());

    svg.attributes
        .insert("height".into(), (h + 2.0 * HALO_PAD * sy).to_string());
}

impl SvgRepo {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            svg_map: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &str) -> Result<&RecordingSurface, SvgRepoError> {
        self.get_extra::<fn() -> Options>(key, None)
    }

    /// Renders `key`'s icon, or returns the surface already cached under it.
    ///
    /// `key` alone identifies the cache entry, but the surface depends on the whole of
    /// [`Options`] - the halo, the boundedness, the stylesheet. So a caller has to fold
    /// into `key` everything it varies: two callers asking for the same name with
    /// different options share one surface, and whichever renders first wins. That is
    /// how an obstacle's glow once came and went with whatever else was on the tile.
    pub fn get_extra<T>(
        &mut self,
        key: &str,
        get_options: Option<T>,
    ) -> Result<&RecordingSurface, SvgRepoError>
    where
        T: FnOnce() -> Options,
    {
        let svg_map = &mut self.svg_map;

        if !svg_map.contains_key(key) {
            let options = get_options.map_or_else(|| Options {
                    names: vec![key.to_string()],
                    ..Default::default()
                }, |get_options| get_options());

            let mut main_svg: Option<Element> = None;

            for ref name in options.names {
                let full_path = self.base.join(format!("{name}.svg"));

                let input = read_to_string(full_path).map_err(|err| SvgRepoError {
                    msg: format!("Error loading SVG ({name})"),
                    source: Some(err.into()),
                })?;

                let mut svg_element =
                    Element::parse(input.as_bytes()).map_err(|err| SvgRepoError {
                        msg: format!("XML parsing error ({name})"),
                        source: Some(err.into()),
                    })?;

                if svg_element.name.split(':').next_back() != Some("svg") {
                    return Err(SvgRepoError {
                        msg: "Expected single <svg> root element".into(),
                        source: None,
                    });
                }

                if let Some(target) = &mut main_svg {
                    target.children.append(&mut svg_element.children);
                } else {
                    main_svg = Some(svg_element);
                }
            }

            let mut main_svg = main_svg.ok_or_else(|| SvgRepoError {
                msg: "No SVGs provided".into(),
                source: None,
            })?;

            if options.halo {
                let element_count = main_svg
                    .children
                    .iter()
                    .filter(|ch| matches!(ch, XMLNode::Element(_)))
                    .count();

                if element_count == 1 {
                    if let Some(XMLNode::Element(el)) = main_svg
                        .children
                        .iter_mut()
                        .find(|ch| matches!(ch, XMLNode::Element(_)))
                    {
                        el.attributes
                            .insert("style".into(), halo_style("stroke-opacity"));
                    }
                } else if element_count > 0 {
                    let mut element_children = Vec::new();
                    let mut other_children = Vec::new();

                    for child in main_svg.children.drain(..) {
                        match child {
                            XMLNode::Element(el) => element_children.push(el),
                            other => other_children.push(other),
                        }
                    }

                    let mut u = Element::new("use");
                    u.attributes.insert("href".into(), "#main".into());
                    u.attributes.insert("style".into(), halo_style("opacity"));

                    let mut g = Element::new("g");
                    g.attributes.insert("id".into(), "main".into());

                    for el in element_children {
                        g.children.push(XMLNode::Element(el));
                    }

                    main_svg.children = other_children;
                    main_svg.children.push(XMLNode::Element(u));
                    main_svg.children.push(XMLNode::Element(g));
                }

                pad_viewport(&mut main_svg);
            }

            let mut svg_bytes = Vec::new();

            main_svg
                .write_with_config(&mut svg_bytes, EmitterConfig::new().perform_indent(true))
                .map_err(|err| SvgRepoError {
                    msg: format!("Error formatting XML ({key})"),
                    source: Some(err.into()),
                })?;

            // println!(
            //     "XXXXXXXXXXXXXXXXXXXXX {key}: {} ||| {:?}",
            //     String::from_utf8(svg_bytes.clone()).unwrap(),
            //     options.stylesheet
            // );

            let bytes = glib::Bytes::from_owned(svg_bytes);

            let stream = gio::MemoryInputStream::from_bytes(&bytes);

            let map_loading_error = |err: LoadingError| SvgRepoError {
                msg: format!("Error loading SVG ({key})"),
                source: Some(err.into()),
            };

            let mut handle = rsvg::Loader::new()
                .read_stream(
                    &stream,
                    None::<&gio::File>, // no base file as this document has no references
                    None::<&gio::Cancellable>, // no cancellable
                )
                .map_err(map_loading_error)?;

            if let Some(stylesheet) = options.stylesheet {
                handle
                    .set_stylesheet(&stylesheet)
                    .map_err(map_loading_error)?;
            }

            let map_cairo_error = |err: cairo::Error| SvgRepoError {
                msg: format!("Cairo error ({key})"),
                source: Some(err.into()),
            };

            let renderer = rsvg::CairoRenderer::new(&handle);

            let dim = renderer.intrinsic_size_in_pixels().unwrap_or((16.0, 16.0));
            let rect = Rectangle::new(0.0, 0.0, dim.0, dim.1);
            let surface = RecordingSurface::create(
                Content::ColorAlpha,
                if options.use_extents {
                    Some(rect)
                } else {
                    None
                },
            )
            .map_err(map_cairo_error)?;
            let context = cairo::Context::new(&surface).map_err(map_cairo_error)?;

            renderer
                .render_document(&context, &rect)
                .map_err(|err| SvgRepoError {
                    msg: format!("Rendering error ({key})"),
                    source: Some(err.into()),
                })?;

            svg_map.insert(key.to_string(), surface);
        }

        Ok(svg_map.get(key).expect("svg from map"))
    }
}
