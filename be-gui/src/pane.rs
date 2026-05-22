use std::collections::HashMap;

use be_input::Direction;
use kurbo::{Axis, Point, Rect, Size};

use crate::{Distance, Layout, RenderStore, ViewId, view::View};

pub enum Pane {
  View(ViewId),
  Split(Split),
}

/// Identifies a split divider within the pane tree.
///
/// `path` is the sequence of child indices to walk from the root `Pane` down
/// to the `Split` that owns the divider. `index` is which gap within that
/// split (between `items[index]` and `items[index + 1]`).
#[derive(Clone, Debug, PartialEq)]
pub struct SplitPath {
  path:     Vec<usize>,
  index:    usize,
  pub axis: Axis,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HitResult {
  View(ViewId),
  Divider(SplitPath),
}

impl HitResult {
  pub fn view_id(&self) -> Option<ViewId> {
    match self {
      HitResult::View(id) => Some(*id),
      HitResult::Divider(_) => None,
    }
  }
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
        let view = views.get_mut(id).expect("pane references view not in views map");
        view.bounds = layout.current_bounds();

        layout.active = Some(*id);
        views.get_mut(id).expect("pane references view not in views map").layout(layout);
        layout.active = None;
      }
      Pane::Split(split) => split.layout(views, layout),
    }
  }

  pub fn hit_pane(&self, pos: Point, size: Size) -> Option<HitResult> {
    self.hit_pane_inner(pos, size, &mut vec![])
  }

  fn hit_pane_inner(&self, pos: Point, size: Size, path: &mut Vec<usize>) -> Option<HitResult> {
    match self {
      Pane::View(id) => Some(HitResult::View(*id)),
      Pane::Split(split) => split.hit_pane(pos, size, path),
    }
  }

  pub fn resize(&mut self, path: &SplitPath, delta: f64, content_size: Size) {
    self.resize_inner(&path.path, path.index, delta, content_size);
  }

  fn resize_inner(&mut self, path: &[usize], index: usize, delta: f64, size: Size) {
    if let Pane::Split(split) = self {
      if path.is_empty() {
        let axis_size = match split.axis {
          Axis::Vertical => size.width,
          Axis::Horizontal => size.height,
        };
        split.resize_divider(index, delta, axis_size);
      } else {
        let child_idx = path[0];
        let percent = split.items[child_idx].0;
        let child_size = match split.axis {
          Axis::Vertical => Size::new(percent * size.width, size.height),
          Axis::Horizontal => Size::new(size.width, percent * size.height),
        };
        split.items[child_idx].1.resize_inner(&path[1..], index, delta, child_size);
      }
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

  pub fn split(
    &mut self,
    axis: Axis,
    views: &mut HashMap<ViewId, View>,
    store: &RenderStore,
    new_view: ViewId,
  ) {
    match self {
      Pane::View(v) => {
        views.get_mut(v).unwrap().on_focus(false);
        views.get_mut(&new_view).unwrap().on_focus(true);
        *self = Pane::Split(Split {
          axis,
          active: 1,
          items: vec![(0.5, Pane::View(*v)), (0.5, Pane::View(new_view))],
        });
      }

      Pane::Split(s) => match &mut s.items[s.active].1 {
        active @ Pane::Split(_) => active.split(axis, views, store, new_view),
        active @ Pane::View(_) if s.axis != axis => active.split(axis, views, store, new_view),
        _ => {
          let fract = s.items.len() as f64 / (s.items.len() + 1) as f64;
          for p in &mut s.items {
            p.0 *= fract;
          }
          s.items.insert(s.active + 1, (1.0 - fract, Pane::View(new_view)));
          views.get_mut(&s.items[s.active].1.active()).unwrap().on_focus(false);
          s.active += 1;
          views.get_mut(&s.items[s.active].1.active()).unwrap().on_focus(true);
        }
      },
    }
  }

  pub fn views(&self) -> ViewsIter<'_> { ViewsIter { stack: vec![self] } }
}

impl Split {
  // TODO: This (layout + hit_view) kinda sucks. However, I believe it is the
  // least bad option, as this lets views own all of their state.

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

  fn hit_pane(&self, pos: Point, size: Size, path: &mut Vec<usize>) -> Option<HitResult> {
    const DIVIDER_HIT: f64 = 4.0;

    let total = match self.axis {
      Axis::Vertical => size.width,
      Axis::Horizontal => size.height,
    };
    let coord = match self.axis {
      Axis::Vertical => pos.x,
      Axis::Horizontal => pos.y,
    };

    let mut offset = 0.0;
    let boundaries: Vec<f64> = self
      .items
      .iter()
      .map(|(p, _)| {
        offset += p * total;
        offset
      })
      .collect();

    // Check dividers before recursing into children.
    for (i, &boundary) in boundaries[..boundaries.len() - 1].iter().enumerate() {
      if (coord - boundary).abs() < DIVIDER_HIT {
        return Some(HitResult::Divider(SplitPath {
          path:  path.clone(),
          index: i,
          axis:  self.axis,
        }));
      }
    }

    let mut prev = 0.0;
    for (i, (&boundary, (_, item))) in boundaries.iter().zip(self.items.iter()).enumerate() {
      if coord < boundary || i == self.items.len() - 1 {
        path.push(i);
        let (child_pos, child_size) = match self.axis {
          Axis::Vertical => {
            (Point::new(pos.x - prev, pos.y), Size::new(boundary - prev, size.height))
          }
          Axis::Horizontal => {
            (Point::new(pos.x, pos.y - prev), Size::new(size.width, boundary - prev))
          }
        };
        let result = item.hit_pane_inner(child_pos, child_size, path);
        path.pop();
        return result;
      }
      prev = boundary;
    }

    None
  }

  fn resize_divider(&mut self, index: usize, delta: f64, axis_size: f64) {
    let delta_pct = delta / axis_size;
    let min = 0.05;
    let new_a = self.items[index].0 + delta_pct;
    let new_b = self.items[index + 1].0 - delta_pct;
    if new_a >= min && new_b >= min {
      self.items[index].0 = new_a;
      self.items[index + 1].0 = new_b;
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
