use std::collections::HashMap;

use be_input::Direction;
use kurbo::{Axis, Point, Rect};

use crate::{Distance, Layout, Render, ViewId, view::View};

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

pub struct ViewsIter<'a> {
  stack: Vec<&'a Pane>,
}

impl Pane {
  pub fn animated(&self, views: &HashMap<ViewId, View>) -> bool {
    match self {
      Pane::View(id) => views[id].animated(),
      Pane::Split(split) => split.items.iter().any(|item| item.animated(views)),
    }
  }

  pub fn draw(&self, views: &mut HashMap<ViewId, View>, render: &mut Render) {
    match self {
      Pane::View(id) => views.get_mut(id).unwrap().draw(render),
      Pane::Split(split) => split.draw(views, render),
    }
  }

  pub fn layout(&self, views: &mut HashMap<ViewId, View>, layout: &mut Layout) {
    match self {
      Pane::View(id) => {
        let view = views.get_mut(id).unwrap();
        view.bounds = layout.current_bounds();
      }
      Pane::Split(split) => split.layout(views, layout),
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

  pub fn views(&self) -> ViewsIter<'_> { ViewsIter { stack: vec![self] } }
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

  fn layout(&self, views: &mut HashMap<ViewId, View>, layout: &mut Layout) {
    let mut bounds = Rect::from_origin_size(Point::ZERO, layout.size());

    match self.axis {
      Axis::Vertical => {
        for (i, item) in self.items.iter().enumerate() {
          let percent =
            self.percent.get(i).copied().unwrap_or_else(|| 1.0 - self.percent.iter().sum::<f64>());
          let mut distance = Distance::Percent(percent).to_pixels_in(layout.size().width);
          if distance < 0.0 {
            distance += layout.size().width;
          }

          bounds.x1 = bounds.x0 + distance;
          layout.clipped(bounds, |layout| item.layout(views, layout));
          bounds.x0 += distance;
        }
      }

      Axis::Horizontal => {
        for (i, item) in self.items.iter().enumerate() {
          let percent =
            self.percent.get(i).copied().unwrap_or_else(|| 1.0 - self.percent.iter().sum::<f64>());
          let mut distance = Distance::Percent(percent).to_pixels_in(layout.size().height);
          if distance < 0.0 {
            distance += layout.size().height;
          }

          bounds.y1 = bounds.y0 + distance;
          layout.clipped(bounds, |layout| item.layout(views, layout));
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

impl Iterator for ViewsIter<'_> {
  type Item = ViewId;

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      match self.stack.pop()? {
        Pane::View(id) => break Some(*id),
        Pane::Split(split) => {
          self.stack.extend(split.items.iter().rev());
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn views_iter() {
    let pane = Pane::Split(Split {
      axis:    Axis::Vertical,
      percent: vec![0.2],
      active:  1,
      items:   vec![Pane::View(ViewId(0)), Pane::View(ViewId(1))],
    });

    assert_eq!(pane.views().collect::<Vec<ViewId>>(), vec![ViewId(0), ViewId(1)]);

    let pane = Pane::Split(Split {
      axis:    Axis::Vertical,
      percent: vec![0.2],
      active:  1,
      items:   vec![
        Pane::Split(Split {
          axis:    Axis::Horizontal,
          percent: vec![0.5],
          active:  1,
          items:   vec![Pane::View(ViewId(0)), Pane::View(ViewId(1))],
        }),
        Pane::View(ViewId(2)),
      ],
    });

    assert_eq!(pane.views().collect::<Vec<ViewId>>(), vec![ViewId(0), ViewId(1), ViewId(2)]);
  }
}
