use std::collections::HashMap;

use be_input::Direction;
use kurbo::{Axis, Point, Rect};

use crate::{Distance, Layout, RenderStore, ViewCollection, ViewId, view::View};

pub enum Pane {
  View(ViewId),
  Split(Split),
}

pub struct Split {
  pub axis:   Axis,
  pub active: usize,
  pub items:  Vec<(f64, Pane)>,
}

pub struct ViewsIter<'a> {
  stack: Vec<&'a Pane>,
}

impl Pane {
  pub fn animated(&self, views: &HashMap<ViewId, View>) -> bool {
    match self {
      Pane::View(id) => views[id].animated(),
      Pane::Split(split) => split.items.iter().any(|item| item.1.animated(views)),
    }
  }

  pub fn layout(&self, views: &mut HashMap<ViewId, View>, layout: &mut Layout) {
    match self {
      Pane::View(id) => {
        let view = views.get_mut(id).unwrap();
        view.bounds = layout.current_bounds();

        layout.active = Some(*id);
        views.get_mut(id).unwrap().layout(layout);
        layout.active = None;
      }
      Pane::Split(split) => split.layout(views, layout),
    }
  }

  pub fn active(&self) -> ViewId {
    match self {
      Pane::View(id) => *id,
      Pane::Split(split) => split.items[split.active].1.active(),
    }
  }

  pub fn focus(&mut self, direction: Direction) -> Option<ViewId> {
    match self {
      Pane::View(_) => None,
      Pane::Split(split) => split.focus(direction),
    }
  }

  pub fn close(&mut self, view: ViewId, views: &mut HashMap<ViewId, View>) {
    match self {
      Pane::View(v) => {
        if view == *v {
          panic!("cannot close a view that is itself");
        }
      }

      Pane::Split(split) => {
        if let Some(idx) =
          split.items.iter().position(|(_, item)| matches!(item, Pane::View(v) if *v == view))
        {
          if split.items.len() == 1 {
            // You can't close the last pane. TODO: Maybe quit the app?
          } else {
            split.active = split.active.saturating_sub(1);

            split.items.remove(idx);

            let total = split.items.iter().map(|(p, _)| p).sum::<f64>();
            for (p, _) in &mut split.items {
              *p /= total;
            }

            views.get_mut(&split.items[split.active].1.active()).unwrap().on_focus(true);
          }
        } else {
          for item in &mut split.items {
            item.1.close(view, views);
          }
        }
      }
    }
  }

  pub fn split(&mut self, axis: Axis, views: &mut ViewCollection, store: &RenderStore) {
    match self {
      Pane::View(v) => {
        let v2 = views.new_view(crate::view::EditorView::new(store));

        views.views.get_mut(v).unwrap().on_focus(false);
        views.views.get_mut(&v2).unwrap().on_focus(true);
        *self = Pane::Split(Split {
          axis,
          active: 1,
          items: vec![(0.5, Pane::View(*v)), (0.5, Pane::View(v2))],
        });
      }

      Pane::Split(s) => match &mut s.items[s.active].1 {
        active @ Pane::Split(_) => active.split(axis, views, store),
        active @ Pane::View(_) if s.axis != axis => active.split(axis, views, store),
        _ => {
          let fract = s.items.len() as f64 / (s.items.len() + 1) as f64;
          for p in &mut s.items {
            p.0 *= fract;
          }
          s.items.insert(
            s.active + 1,
            (1.0 - fract, Pane::View(views.new_view(crate::view::EditorView::new(store)))),
          );
          views.views.get_mut(&s.items[s.active].1.active()).unwrap().on_focus(false);
          s.active += 1;
          views.views.get_mut(&s.items[s.active].1.active()).unwrap().on_focus(true);
        }
      },
    }
  }

  pub fn views(&self) -> ViewsIter<'_> { ViewsIter { stack: vec![self] } }
}

impl Split {
  fn layout(&self, views: &mut HashMap<ViewId, View>, layout: &mut Layout) {
    let mut bounds = Rect::from_origin_size(Point::ZERO, layout.size());

    match self.axis {
      Axis::Vertical => {
        for (percent, item) in self.items.iter() {
          let mut distance = Distance::Percent(*percent).to_pixels_in(layout.size().width);
          if distance < 0.0 {
            distance += layout.size().width;
          }

          bounds.x1 = bounds.x0 + distance;
          layout.clipped(bounds, |layout| item.layout(views, layout));
          bounds.x0 += distance;
        }
      }

      Axis::Horizontal => {
        for (percent, item) in self.items.iter() {
          let mut distance = Distance::Percent(*percent).to_pixels_in(layout.size().height);
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

    if let Some(view) = focused.1.focus(direction) {
      Some(view)
    } else {
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

      Some(self.items[self.active].1.active())
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
          self.stack.extend(split.items.iter().map(|(_, item)| item).rev());
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
      axis:   Axis::Vertical,
      active: 1,
      items:  vec![(0.2, Pane::View(ViewId(0))), (0.8, Pane::View(ViewId(1)))],
    });

    assert_eq!(pane.views().collect::<Vec<ViewId>>(), vec![ViewId(0), ViewId(1)]);

    let pane = Pane::Split(Split {
      axis:   Axis::Vertical,
      active: 1,
      items:  vec![
        (
          0.2,
          Pane::Split(Split {
            axis:   Axis::Horizontal,
            active: 1,
            items:  vec![(0.5, Pane::View(ViewId(0))), (0.5, Pane::View(ViewId(1)))],
          }),
        ),
        (0.8, Pane::View(ViewId(2))),
      ],
    });

    assert_eq!(pane.views().collect::<Vec<ViewId>>(), vec![ViewId(0), ViewId(1), ViewId(2)]);
  }
}
