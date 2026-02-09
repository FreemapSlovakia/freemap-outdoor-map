use crate::render::colors::ContextExt;
use cairo::Context;
use geo::{Coord, Intersects, Rect};

const DEBUG: bool = false;

pub struct Collision<'a> {
    items: Vec<Rect>,
    context: Option<&'a Context>,
}

const EPSILON: f64 = 0.001;

impl<'a> Collision<'a> {
    pub const fn new(context: Option<&'a Context>) -> Self {
        Self {
            items: vec![],
            context,
        }
    }

    pub fn add(&mut self, item: Rect) -> usize {
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

        if DEBUG && let Some(context) = self.context {
            context.rectangle(item.min().x, item.min().y, item.width(), item.height());

            context.save().expect("context saved");
            context.set_source_rgba(0.0, 1.0, 0.0, 0.5);
            context.set_line_width(1.0);
            context.stroke().unwrap();
            context.restore().expect("context restored");
        }

        self.items.len() - 1
    }

    pub fn collides(&self, bb: &Rect) -> bool {
        let _span = tracy_client::span!("collision::collides");

        let intersects = self.items.iter().any(|item| bb.intersects(item));

        if DEBUG
            && intersects
            && let Some(context) = self.context
        {
            context.rectangle(bb.min().x, bb.min().y, bb.width(), bb.height());

            context.save().expect("context saved");
            context.set_source_rgba(1.0, 0.0, 0.0, 0.2);
            context.set_line_width(1.0);
            context.stroke().unwrap();
            context.restore().expect("context restored");
        }

        intersects
    }

    pub fn collides_with_exclusion(&self, bbox: &Rect, exclude: usize) -> bool {
        let _span = tracy_client::span!("collision::collides");

        self.items
            .iter()
            .enumerate()
            .any(|(idx, item)| idx != exclude && bbox.intersects(item))
    }
}
