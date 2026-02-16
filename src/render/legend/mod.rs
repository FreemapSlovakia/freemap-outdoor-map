mod ctx_ext;
mod default;
mod feature_lines;
mod landcovers;
mod mapping;
mod pois;
mod roads;
mod shared;

use crate::render::layers::Category;
use crate::render::{ImageFormat, RenderRequest};
use geo::{Coord, Rect};
use indexmap::IndexMap;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::OnceLock;

pub(crate) use shared::LegendItemData;

use serde::Deserialize;

#[derive(Deserialize, PartialEq, Clone, Copy)]
pub enum LegendMode {
    Normal,
    TagInfo,
}

#[derive(Clone, Serialize)]
pub struct LegendMeta<'a> {
    pub id: &'a str,
    pub category: Category,
    pub tags: Vec<IndexMap<&'a str, &'a str>>,
}

pub(super) struct LegendItem<'a> {
    pub(super) meta: LegendMeta<'a>,
    pub(super) zoom: u8,
    pub(super) data: LegendItemData,
}

impl<'a> LegendItem<'a> {
    pub(super) fn new(
        id: &'static str,
        category: Category,
        tags: impl Into<Vec<IndexMap<&'static str, &'static str>>>,
        data: LegendItemData,
        zoom: u8,
    ) -> Self {
        Self {
            meta: LegendMeta {
                id,
                category,
                tags: tags.into(),
            },
            data,
            zoom,
        }
    }
}

static MAPPING_PATH: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_mapping_path(path: PathBuf) {
    if MAPPING_PATH.set(path).is_err() {
        panic!("mapping path already set");
    }
}

pub(super) fn mapping_path() -> &'static PathBuf {
    MAPPING_PATH
        .get()
        .expect("mapping path must be set before legend use")
}

static LEGEND_ITEMS: LazyLock<Vec<LegendItem>> =
    LazyLock::new(|| default::build_legend_items(LegendMode::Normal));

static LEGEND_ITEMS_FOR_TAGINFO: LazyLock<Vec<LegendItem>> =
    LazyLock::new(|| default::build_legend_items(LegendMode::TagInfo));

pub fn legend_metadata() -> Vec<LegendMeta<'static>> {
    LEGEND_ITEMS.iter().map(|item| item.meta.clone()).collect()
}

pub fn legend_render_request(id: &str, scale: f64, mode: LegendMode) -> Option<RenderRequest> {
    let items = match mode {
        LegendMode::Normal => &LEGEND_ITEMS,
        LegendMode::TagInfo => &LEGEND_ITEMS_FOR_TAGINFO,
    };

    let (legend_item_data, zoom) = items
        .iter()
        .find(|item| item.meta.id == id)
        .map(|item| (item.data.clone(), item.zoom))?;

    let zoom_factor = (20f64 - zoom as f64).exp2();

    let bbox = Rect::new(
        Coord {
            x: -8.0 * zoom_factor,
            y: -3.5 * zoom_factor,
        },
        Coord {
            x: 8.0 * zoom_factor,
            y: 3.5 * zoom_factor,
        },
    );

    let mut render_request = RenderRequest::new(bbox, zoom, scale, ImageFormat::Png);

    render_request.legend = Some(legend_item_data);

    Some(render_request)
}
