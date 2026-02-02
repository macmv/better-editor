use kurbo::{Axis, Point, Rect, Size, Vec2};

use crate::{Distance, Notify, RenderStore, ViewId};

pub struct Layout<'a> {
  pub store: &'a mut RenderStore,

  scale: f64,
  size:  Size,

  stack: Vec<Rect>,

  pub active:   Option<ViewId>,
  pub to_close: Vec<ViewId>,
}

impl<'a> Layout<'a> {
  pub fn new(store: &'a mut RenderStore, scale: f64, size: Size) -> Self {
    Self { store, scale, size, stack: vec![], active: None, to_close: vec![] }
  }
}

impl<'a> Layout<'a> {
  pub fn size(&self) -> Size {
    if let Some(top) = self.stack.last() { top.size() } else { self.size }
  }

  fn offset(&self) -> Vec2 {
    if let Some(top) = self.stack.last() { top.origin().to_vec2() } else { Vec2::ZERO }
  }

  pub fn notifier(&self) -> Notify { self.store.notifier() }

  pub fn current_bounds(&self) -> Rect {
    Rect::from_origin_size(self.offset().to_point(), self.size())
  }

  pub fn split<S>(
    &mut self,
    state: &mut S,
    axis: Axis,
    distance: Distance,
    left: impl FnOnce(&mut S, &mut Layout),
    right: impl FnOnce(&mut S, &mut Layout),
  ) {
    let mut left_bounds = Rect::from_origin_size(Point::ZERO, self.size());
    let mut right_bounds = Rect::from_origin_size(Point::ZERO, self.size());

    match axis {
      Axis::Vertical => {
        let mut distance = distance.to_pixels_in(self.size().width);
        if distance < 0.0 {
          distance += self.size().width;
        }

        left_bounds.x1 = distance;
        right_bounds.x0 = distance;
      }
      Axis::Horizontal => {
        let mut distance = distance.to_pixels_in(self.size().height);
        if distance < 0.0 {
          distance += self.size().height;
        }

        left_bounds.y1 = distance;
        right_bounds.y0 = distance;
      }
    }

    self.clipped(left_bounds, |render| left(state, render));
    self.clipped(right_bounds, |render| right(state, render));
  }

  pub fn clipped(&mut self, mut rect: Rect, f: impl FnOnce(&mut Layout)) {
    rect = rect + self.offset();

    let scaled_rect = rect.scale_from_origin(self.scale).round();
    self.stack.push(scaled_rect.scale_from_origin(1.0 / self.scale));

    f(self);

    self.stack.pop().expect("no clip layer to pop");
  }

  pub fn close_view(&mut self) {
    if let Some(id) = self.active {
      self.to_close.push(id);
    } else {
      panic!("no active view set");
    }
  }
}
