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
#[error("{msg}{}", source.as_deref().map_or("".to_string(), |err| format!(": {err}")))]
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
            let options = get_options
                .map(|get_options| get_options())
                .unwrap_or_else(|| Options {
                    names: vec![key.to_string()],
                    ..Default::default()
                });

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
                        el.attributes.insert(
                            "style".into(),
                            concat!(
                                "stroke:#fff;",
                                "stroke-width:3;",
                                "stroke-opacity:0.5;",
                                "stroke-linejoin:round;",
                                "paint-order:stroke"
                            )
                            .into(),
                        );
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
                    u.attributes.insert(
                        "style".into(),
                        concat!(
                            "stroke:#fff;",
                            "stroke-width:3;",
                            "opacity:0.5;",
                            "stroke-linejoin:round;",
                            "paint-order:stroke"
                        )
                        .into(),
                    );

                    let mut g = Element::new("g");
                    g.attributes.insert("id".into(), "main".into());

                    for el in element_children {
                        g.children.push(XMLNode::Element(el));
                    }

                    main_svg.children = other_children;
                    main_svg.children.push(XMLNode::Element(u));
                    main_svg.children.push(XMLNode::Element(g));
                }
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
