use std::collections::HashMap;

use be_input::Direction;
use kurbo::{Axis, Point, Rect};

use crate::{Distance, Render, ViewId, view::View};

pub enum Pane {
  View(ViewId),
  Split(Split),
}

pub struct Split {
  pub axis:    Axis,
  pub percent: Vec<f64>,
  pub active:  usize,
  pub items:   Vec<Pane>,
}

impl Pane {
  pub fn draw(&self, views: &mut HashMap<ViewId, View>, render: &mut Render) {
    match self {
      Pane::View(id) => views.get_mut(id).unwrap().draw(render),
      Pane::Split(split) => split.draw(views, render),
    }
  }

  pub fn active(&self) -> ViewId {
    match self {
      Pane::View(id) => *id,
      Pane::Split(split) => split.items[split.active].active(),
    }
  }

  pub fn focus(&mut self, direction: Direction) -> Option<ViewId> {
    match self {
      Pane::View(_) => None,
      Pane::Split(split) => split.focus(direction),
    }
  }
}

impl Split {
  fn draw(&self, views: &mut HashMap<ViewId, View>, render: &mut Render) {
    let mut bounds = Rect::from_origin_size(Point::ZERO, render.size());

    match self.axis {
      Axis::Vertical => {
        for (i, item) in self.items.iter().enumerate() {
          let percent =
            self.percent.get(i).copied().unwrap_or_else(|| 1.0 - self.percent.iter().sum::<f64>());
          let mut distance = Distance::Percent(percent).to_pixels_in(render.size().width);
          if distance < 0.0 {
            distance += render.size().width;
          }

          bounds.x1 = bounds.x0 + distance;
          render.clipped(bounds, |render| item.draw(views, render));
          bounds.x0 += distance;
        }
      }

      Axis::Horizontal => {
        for (i, item) in self.items.iter().enumerate() {
          let percent =
            self.percent.get(i).copied().unwrap_or_else(|| 1.0 - self.percent.iter().sum::<f64>());
          let mut distance = Distance::Percent(percent).to_pixels_in(render.size().height);
          if distance < 0.0 {
            distance += render.size().height;
          }

          bounds.y1 = bounds.y0 + distance;
          render.clipped(bounds, |render| item.draw(views, render));
          bounds.y0 += distance;
        }
      }
    }
  }

  /// Returns true if the focus changed.
  fn focus(&mut self, direction: Direction) -> Option<ViewId> {
    let focused = &mut self.items[self.active];

    if focused.focus(direction).is_none() {
      match (self.axis, direction) {
        (Axis::Vertical, Direction::Right) if self.active < self.items.len() - 1 => {
          self.active += 1
        }
        (Axis::Vertical, Direction::Left) if self.active > 0 => self.active -= 1,
        (Axis::Horizontal, Direction::Down) if self.active < self.items.len() - 1 => {
          self.active += 1
        }
        (Axis::Horizontal, Direction::Up) if self.active > 0 => self.active -= 1,

        _ => return None,
      }

      Some(self.items[self.active].active())
    } else {
      None
    }
  }
}
