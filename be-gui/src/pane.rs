use std::collections::HashMap;

use be_input::Direction;
use kurbo::{Axis, Point, Rect};

use crate::{Distance, Layout, RenderStore, ViewCollection, ViewId, view::View};

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
      Pane::Split(split) => split.items[split.active].active(),
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
          split.items.iter().position(|item| matches!(item, Pane::View(v) if *v == view))
        {
          if split.items.len() == 2 {
            split.items.remove(idx);
            *self = split.items.pop().unwrap();

            views.get_mut(&self.active()).unwrap().on_focus(true);
          } else {
            // TODO: Even out the percentages.
            if idx == split.items.len() - 1 {
              split.percent.pop();
            } else {
              split.percent.remove(idx);
            }
            split.active = split.active.saturating_sub(1);

            split.items.remove(idx);
            views.get_mut(&split.items[split.active].active()).unwrap().on_focus(true);
          }
        } else {
          for item in &mut split.items {
            item.close(view, views);
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
          percent: vec![0.5],
          active: 1,
          items: vec![Pane::View(*v), Pane::View(v2)],
        });
      }

      Pane::Split(s) => match &mut s.items[s.active] {
        active @ Pane::Split(_) => active.split(axis, views, store),
        active @ Pane::View(_) if s.axis != axis => active.split(axis, views, store),
        _ => {
          let fract = s.items.len() as f64 / (s.items.len() + 1) as f64;
          s.items
            .insert(s.active + 1, Pane::View(views.new_view(crate::view::EditorView::new(store))));
          views.views.get_mut(&s.items[s.active].active()).unwrap().on_focus(false);
          s.active += 1;
          views.views.get_mut(&s.items[s.active].active()).unwrap().on_focus(true);
          for p in &mut s.percent {
            *p *= fract;
          }
          s.percent.push(1.0 - fract);
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

    if let Some(view) = focused.focus(direction) {
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

      Some(self.items[self.active].active())
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
