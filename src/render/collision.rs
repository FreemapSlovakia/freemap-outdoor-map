use cairo::Context;
use geo::{Coord, Intersects, Rect};
use rustc_hash::FxHashMap;

const DEBUG: bool = false;

const EPSILON: f64 = 0.001;

/// Side of a grid cell in pixels; most boxes are glyphs and icons spanning a cell or two.
const CELL_SIZE: f64 = 64.0;

/// Boxes spanning more cells stay out of the grid and are checked on every query.
const MAX_CELLS: f64 = 64.0;

/// Below this many boxes a linear scan is cheaper than the grid.
const GRID_MIN_ITEMS: usize = 32;

pub struct Collision<'ctx> {
    items: Vec<Rect>,
    cells: FxHashMap<(i32, i32), Vec<usize>>,
    oversized: Vec<usize>,
    context: Option<&'ctx Context>,
}

/// Grid cells `rect` touches, or `None` when it spans too many or isn't finite.
fn cell_range(rect: &Rect) -> Option<(i32, i32, i32, i32)> {
    let cell = |v: f64| (v / CELL_SIZE).floor();

    let (x0, y0) = (cell(rect.min().x), cell(rect.min().y));
    let (x1, y1) = (cell(rect.max().x), cell(rect.max().y));

    // Casts saturate, so far-off boxes merely share edge cells; `hits` still tests exactly.
    ((x1 - x0 + 1.0) * (y1 - y0 + 1.0) <= MAX_CELLS).then_some((
        x0 as i32, y0 as i32, x1 as i32, y1 as i32,
    ))
}

impl<'a> Collision<'a> {
    pub fn new(context: Option<&'a Context>) -> Self {
        Self {
            items: vec![],
            cells: FxHashMap::default(),
            oversized: vec![],
            context,
        }
    }

    pub fn add(&mut self, item: Rect) -> usize {
        let idx = self.items.len();

        self.items.push(Rect::new(
            Coord {
                x: item.min().x - EPSILON,
                y: item.min().y - EPSILON,
            },
            Coord {
                x: item.max().x + EPSILON,
                y: item.max().y + EPSILON,
            },
        ));

        match self.items.len() {
            GRID_MIN_ITEMS => (0..GRID_MIN_ITEMS).for_each(|i| self.index(i)),
            len if len > GRID_MIN_ITEMS => self.index(idx),
            _ => {}
        }

        if DEBUG && let Some(context) = self.context {
            context.rectangle(item.min().x, item.min().y, item.width(), item.height());

            context.save().expect("context saved");
            context.set_source_rgba(0.0, 0.5, 0.0, 0.5);
            context.set_line_width(1.0);
            context.stroke().expect("context stroked");
            context.restore().expect("context restored");
        }

        idx
    }

    fn index(&mut self, idx: usize) {
        match cell_range(&self.items[idx]) {
            Some((x0, y0, x1, y1)) => {
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        self.cells.entry((x, y)).or_default().push(idx);
                    }
                }
            }
            None => self.oversized.push(idx),
        }
    }

    /// Whether `bb` intersects any added box other than the one at index `exclude`.
    pub fn collides(&self, bb: &Rect, exclude: Option<usize>) -> bool {
        let _span = tracy_client::span!("collision::collides");

        let intersects = self.hits(bb, exclude);

        if DEBUG
            && intersects
            && let Some(context) = self.context
        {
            context.rectangle(bb.min().x, bb.min().y, bb.width(), bb.height());

            context.save().expect("context saved");
            context.set_source_rgba(1.0, 0.0, 0.0, 0.2);
            context.set_line_width(1.0);
            context.stroke().expect("context stroked");
            context.restore().expect("context restored");
        }

        intersects
    }

    fn hits(&self, bb: &Rect, exclude: Option<usize>) -> bool {
        let accept = |idx: usize| Some(idx) != exclude;

        let range = (self.items.len() >= GRID_MIN_ITEMS)
            .then(|| cell_range(bb))
            .flatten();

        let Some((x0, y0, x1, y1)) = range else {
            return self
                .items
                .iter()
                .enumerate()
                .any(|(idx, item)| accept(idx) && bb.intersects(item));
        };

        self.oversized
            .iter()
            .chain(
                (x0..=x1)
                    .flat_map(|x| (y0..=y1).map(move |y| (x, y)))
                    .filter_map(|key| self.cells.get(&key))
                    .flatten(),
            )
            .any(|&idx| accept(idx) && bb.intersects(&self.items[idx]))
    }
}
