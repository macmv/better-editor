use std::collections::HashMap;

use be_input::Direction;
use kurbo::{Arc, Axis, BezPath, Point, Rect, Size, Vec2};

use crate::{Layout, RenderStore, ViewId, render::Render, view::View};

/// The width of the divider between split panes, in pixels.
pub const DIVIDER_WIDTH: f64 = 4.0;

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
      let num_dividers = (split.items.len().saturating_sub(1)) as f64;
      if path.is_empty() {
        let axis_size = match split.axis {
          Axis::Vertical => size.width - num_dividers * DIVIDER_WIDTH,
          Axis::Horizontal => size.height - num_dividers * DIVIDER_WIDTH,
        };
        split.resize_divider(index, delta, axis_size);
      } else {
        let child_idx = path[0];
        let percent = split.items[child_idx].0;
        let child_size = match split.axis {
          Axis::Vertical => {
            let usable = size.width - num_dividers * DIVIDER_WIDTH;
            Size::new(percent * usable, size.height)
          }
          Axis::Horizontal => {
            let usable = size.height - num_dividers * DIVIDER_WIDTH;
            Size::new(size.width, percent * usable)
          }
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

  /// Returns the active `ViewId` of the pane immediately adjacent to `view`
  /// in the split tree, or `None` if `view` is not found or has no sibling.
  pub fn neighbor_of(&self, view: ViewId) -> Option<ViewId> {
    match self {
      Pane::View(_) => None,
      Pane::Split(split) => {
        // Check if any direct child contains `view`.
        let pos = split.items.iter().position(|(_, pane)| pane.contains(view));
        if let Some(idx) = pos {
          // Pick the sibling immediately after, or before if idx is last.
          let sibling_idx =
            if idx + 1 < split.items.len() { idx + 1 } else { idx.saturating_sub(1) };
          if sibling_idx != idx {
            return Some(split.items[sibling_idx].1.active());
          }
        }
        // Recurse into children.
        for (_, pane) in &split.items {
          if let Some(id) = pane.neighbor_of(view) {
            return Some(id);
          }
        }
        None
      }
    }
  }

  /// Returns true if this pane is or contains `view`.
  fn contains(&self, view: ViewId) -> bool {
    match self {
      Pane::View(id) => *id == view,
      Pane::Split(split) => split.items.iter().any(|(_, pane)| pane.contains(view)),
    }
  }

  pub fn focus(&mut self, direction: Direction) -> Option<ViewId> {
    match self {
      Pane::View(_) => None,
      Pane::Split(split) => split.focus(direction),
    }
  }

  /// Sets active indices at every level of the tree so that `view` becomes the
  /// active leaf.  Returns true if the view was found.
  pub fn focus_view(&mut self, view: ViewId) -> bool {
    match self {
      Pane::View(id) => *id == view,
      Pane::Split(split) => {
        for (i, (_, pane)) in split.items.iter_mut().enumerate() {
          if pane.focus_view(view) {
            split.active = i;
            return true;
          }
        }
        false
      }
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

  pub fn draw_dividers(&self, render: &mut Render, offset: Point, size: Size) {
    const RADIUS: f64 = 8.0;

    fn bevel(origin: impl Into<Point>, direction: Vec2) -> BezPath {
      let origin = origin.into();

      let (start, sweep): (f64, f64) = if direction.x == 1.0 && direction.y == 1.0 {
        (180.0, 90.0)
      } else if direction.x == -1.0 && direction.y == 1.0 {
        (0.0, -90.0)
      } else if direction.x == 1.0 && direction.y == -1.0 {
        (180.0, -90.0)
      } else {
        (0.0, 90.0)
      };

      let mut bevel = BezPath::new();
      bevel.move_to((origin.x + RADIUS * direction.x, origin.y));
      bevel.line_to((origin.x, origin.y));
      bevel.line_to((origin.x, origin.y + RADIUS * direction.y));
      bevel.extend(
        Arc::new(
          (origin.x + RADIUS * direction.x, origin.y + RADIUS * direction.y),
          Vec2::splat(RADIUS),
          start.to_radians(),
          sweep.to_radians(),
          0.0,
        )
        .append_iter(0.1),
      );
      return bevel;
    }

    let color = render.theme().background_lower;

    if let Pane::Split(split) = self {
      let num_dividers = (split.items.len().saturating_sub(1)) as f64;
      match split.axis {
        Axis::Vertical => {
          let usable = size.width - num_dividers * DIVIDER_WIDTH;
          let mut raw_x = 0.0;
          let mut x = offset.x;
          for (i, (percent, item)) in split.items.iter().enumerate() {
            let item_width = percent * usable;
            raw_x += item_width;
            if i < split.items.len() - 1 {
              let center = (offset.x + raw_x + DIVIDER_WIDTH / 2.0).round();
              let x0 = center - DIVIDER_WIDTH / 2.0;
              let x1 = center + DIVIDER_WIDTH / 2.0;

              item.draw_dividers(render, Point::new(x, offset.y), Size::new(x0 - x, size.height));
              render.fill(&Rect::new(x0, offset.y, x1, offset.y + size.height), color);

              render.fill(&bevel((x0, offset.y), Vec2::new(-1.0, 1.0)), color);
              render.fill(&bevel((x1, offset.y), Vec2::new(1.0, 1.0)), color);
              render.fill(&bevel((x0, offset.y + size.height), Vec2::new(-1.0, -1.0)), color);
              render.fill(&bevel((x1, offset.y + size.height), Vec2::new(1.0, -1.0)), color);

              x = x1;
              raw_x += DIVIDER_WIDTH;
            } else {
              item.draw_dividers(
                render,
                Point::new(x, offset.y),
                Size::new(offset.x + size.width - x, size.height),
              );
            }
          }
        }
        Axis::Horizontal => {
          let usable = size.height - num_dividers * DIVIDER_WIDTH;
          let mut raw_y = 0.0;
          let mut y = offset.y;
          for (i, (percent, item)) in split.items.iter().enumerate() {
            let item_height = percent * usable;
            raw_y += item_height;
            if i < split.items.len() - 1 {
              let center = (offset.y + raw_y + DIVIDER_WIDTH / 2.0).round();
              let y0 = center - DIVIDER_WIDTH / 2.0;
              let y1 = center + DIVIDER_WIDTH / 2.0;

              item.draw_dividers(render, Point::new(offset.x, y), Size::new(size.width, y0 - y));
              render.fill(&Rect::new(offset.x, y0, offset.x + size.width, y1), color);

              render.fill(&bevel((offset.x, y0), Vec2::new(1.0, -1.0)), color);
              render.fill(&bevel((offset.x + size.width, y0), Vec2::new(-1.0, -1.0)), color);
              render.fill(&bevel((offset.x, y1), Vec2::new(1.0, 1.0)), color);
              render.fill(&bevel((offset.x + size.width, y1), Vec2::new(-1.0, 1.0)), color);

              y = y1;
              raw_y += DIVIDER_WIDTH;
            } else {
              item.draw_dividers(
                render,
                Point::new(offset.x, y),
                Size::new(size.width, offset.y + size.height - y),
              );
            }
          }
        }
      }
    }
  }

  pub fn views(&self) -> ViewsIter<'_> { ViewsIter { stack: vec![self] } }
}

impl Split {
  // TODO: This (layout + hit_view) kinda sucks. However, I believe it is the
  // least bad option, as this lets views own all of their state.

  fn layout(&self, views: &mut HashMap<ViewId, View>, layout: &mut Layout) {
    let mut bounds = Rect::from_origin_size(Point::ZERO, layout.size());
    let num_dividers = (self.items.len().saturating_sub(1)) as f64;

    match self.axis {
      Axis::Vertical => {
        let usable = layout.size().width - num_dividers * DIVIDER_WIDTH;
        let mut raw_x = 0.0;
        for (i, (percent, item)) in self.items.iter().enumerate() {
          let distance = percent * usable;
          raw_x += distance;

          if i < self.items.len() - 1 {
            let center = (raw_x + DIVIDER_WIDTH / 2.0).round();
            bounds.x1 = center - DIVIDER_WIDTH / 2.0;
            layout.clipped(bounds, |layout| item.layout(views, layout));
            bounds.x0 = center + DIVIDER_WIDTH / 2.0;
            raw_x += DIVIDER_WIDTH;
          } else {
            bounds.x1 = layout.size().width;
            layout.clipped(bounds, |layout| item.layout(views, layout));
          }
        }
      }

      Axis::Horizontal => {
        let usable = layout.size().height - num_dividers * DIVIDER_WIDTH;
        let mut raw_y = 0.0;
        for (i, (percent, item)) in self.items.iter().enumerate() {
          let distance = percent * usable;
          raw_y += distance;

          if i < self.items.len() - 1 {
            let center = (raw_y + DIVIDER_WIDTH / 2.0).round();
            bounds.y1 = center - DIVIDER_WIDTH / 2.0;
            layout.clipped(bounds, |layout| item.layout(views, layout));
            bounds.y0 = center + DIVIDER_WIDTH / 2.0;
            raw_y += DIVIDER_WIDTH;
          } else {
            bounds.y1 = layout.size().height;
            layout.clipped(bounds, |layout| item.layout(views, layout));
          }
        }
      }
    }
  }

  fn hit_pane(&self, pos: Point, size: Size, path: &mut Vec<usize>) -> Option<HitResult> {
    const HIT_WIDTH: f64 = 8.0;

    let total = match self.axis {
      Axis::Vertical => size.width,
      Axis::Horizontal => size.height,
    };
    let coord = match self.axis {
      Axis::Vertical => pos.x,
      Axis::Horizontal => pos.y,
    };

    let num_dividers = (self.items.len().saturating_sub(1)) as f64;
    let usable = total - num_dividers * DIVIDER_WIDTH;

    // Compute the start and end of each item region, accounting for divider
    // gaps.
    let mut offset = 0.0;
    let mut regions: Vec<(f64, f64)> = Vec::new();
    for (i, (p, _)) in self.items.iter().enumerate() {
      let item_size = p * usable;
      let start = offset;
      let end = offset + item_size;
      regions.push((start, end));
      offset = end;
      if i < self.items.len() - 1 {
        offset += DIVIDER_WIDTH;
      }
    }

    // Check dividers first (the gaps between regions), with an extended hit
    // zone.
    let hit_margin = (HIT_WIDTH - DIVIDER_WIDTH) / 2.0;
    for (i, _) in regions[..regions.len() - 1].iter().enumerate() {
      let divider_start = regions[i].1 - hit_margin;
      let divider_end = regions[i + 1].0 + hit_margin;
      if coord >= divider_start && coord < divider_end {
        return Some(HitResult::Divider(SplitPath {
          path:  path.clone(),
          index: i,
          axis:  self.axis,
        }));
      }
    }

    // Recurse into the child that contains the coordinate.
    for (i, ((start, end), (_, item))) in regions.iter().zip(self.items.iter()).enumerate() {
      if coord < *end || i == self.items.len() - 1 {
        path.push(i);
        let (child_pos, child_size) = match self.axis {
          Axis::Vertical => (Point::new(pos.x - start, pos.y), Size::new(end - start, size.height)),
          Axis::Horizontal => {
            (Point::new(pos.x, pos.y - start), Size::new(size.width, end - start))
          }
        };
        let result = item.hit_pane_inner(child_pos, child_size, path);
        path.pop();
        return result;
      }
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
