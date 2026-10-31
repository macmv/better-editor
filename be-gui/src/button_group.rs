//! A row of icon buttons drawn as one rounded rectangle, split by thin lines
//! (Blender style).

use std::sync::LazyLock;

use kurbo::{Affine, Line, Point, Rect, RoundedRect, RoundedRectRadii, Size, Stroke, Vec2};

use crate::{Render, icon::Icon};

/// The size of each (square) button. This matches a tab button: the tab bar
/// less its margin.
pub const SIZE: f64 = crate::TAB_BAR_HEIGHT - 10.0;
/// The gap between a group and the window edges.
pub const MARGIN: f64 = 5.0;

const RADIUS: f64 = 4.0;
const ICON_SIZE: f64 = 14.0;

/// The width of a group of `count` buttons.
pub const fn width(count: usize) -> f64 { SIZE * count as f64 }

pub struct Item<'a, T> {
  /// Names the button in `hit` and in the hover passed to `draw`.
  pub id:     T,
  pub icon:   &'a LazyLock<Icon>,
  /// Fills the button, for something that is currently open.
  pub active: bool,
}

pub struct ButtonGroup<'a, T> {
  items:  Vec<Item<'a, T>>,
  bounds: Rect,
}

impl<'a, T: Copy + PartialEq> ButtonGroup<'a, T> {
  /// A group in the bottom right of a window of `window`, with `items` from
  /// left to right.
  pub fn bottom_right(window: Size, items: Vec<Item<'a, T>>) -> Self {
    let size = Size::new(width(items.len()), SIZE);
    let origin =
      Point::new(window.width - MARGIN - size.width, window.height - MARGIN - size.height);

    ButtonGroup { items, bounds: Rect::from_origin_size(origin, size) }
  }

  /// The button at `pos`, if any.
  pub fn hit(&self, pos: Point) -> Option<T> {
    if !self.bounds.contains(pos) {
      return None;
    }

    let i = ((pos.x - self.bounds.x0) / SIZE) as usize;
    self.items.get(i).map(|item| item.id)
  }

  /// Draws the group, outlining the `hover` button.
  pub fn draw(&self, render: &mut Render, hover: Option<T>) {
    if self.items.is_empty() {
      return;
    }

    let outline = render.theme().background_raised_outline;
    let last = self.items.len() - 1;

    render.clipped(self.bounds, |render| {
      let shape = Rect::from_origin_size(Point::ZERO, self.bounds.size());
      render.fill(&shape.to_rounded_rect(RADIUS), render.theme().background_raised);

      for (i, item) in self.items.iter().enumerate() {
        let segment =
          Rect::from_origin_size(Point::new(i as f64 * SIZE, 0.0), Size::new(SIZE, SIZE));

        // Only the outer corners of the group are rounded.
        let radii = RoundedRectRadii::new(
          if i == 0 { RADIUS } else { 0.0 },
          if i == last { RADIUS } else { 0.0 },
          if i == last { RADIUS } else { 0.0 },
          if i == 0 { RADIUS } else { 0.0 },
        );

        if item.active {
          render.fill(&RoundedRect::from_rect(segment, radii), outline);
        }
        if item.active || hover == Some(item.id) {
          render.stroke_transform(
            &RoundedRect::from_rect(segment.inset(-0.5), radii),
            Affine::IDENTITY,
            outline,
            Stroke::new(1.0),
          );
        }

        let color = render.theme().text;
        item.icon.stroke(
          segment.origin() + Vec2::splat((SIZE - ICON_SIZE) / 2.0),
          ICON_SIZE,
          color,
          render,
        );
      }

      // The lines between the buttons.
      for i in 1..self.items.len() {
        let x = i as f64 * SIZE;
        render.stroke_transform(
          &Line::new((x, 0.0), (x, SIZE)),
          Affine::IDENTITY,
          outline,
          Stroke::new(1.0),
        );
      }
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hit_finds_the_button_under_the_mouse() {
    let icon = crate::icon::MENU;
    let items = vec![
      Item { id: 'a', icon: &icon, active: false },
      Item { id: 'b', icon: &icon, active: false },
    ];
    let group = ButtonGroup::bottom_right(Size::new(200.0, 100.0), items);

    let right = 200.0 - MARGIN;
    let bottom = 100.0 - MARGIN - SIZE / 2.0;
    assert_eq!(group.hit(Point::new(right - 1.0, bottom)), Some('b'));
    assert_eq!(group.hit(Point::new(right - SIZE - 1.0, bottom)), Some('a'));
    assert_eq!(group.hit(Point::new(right - SIZE * 2.0 - 1.0, bottom)), None);
    assert_eq!(group.hit(Point::new(right + 1.0, bottom)), None);
    assert_eq!(group.hit(Point::new(right - 1.0, 0.0)), None);
  }
}
