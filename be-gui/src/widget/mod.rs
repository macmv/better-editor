use kurbo::{Rect, Size};

mod button;

pub use button::Button;

use crate::{Layout, Render, WidgetPath};

pub struct WidgetStore {
  pub content: Box<dyn Widget>,
  pub bounds:  Rect,
  pub path:    WidgetPath,
}

pub trait Widget {
  fn layout(&mut self, layout: &mut Layout) -> Option<Size> {
    let _ = layout;
    None
  }

  fn draw(&mut self, render: &mut Render);
}

impl WidgetStore {
  pub fn new(path: WidgetPath, content: impl Widget + 'static) -> Self {
    WidgetStore { content: Box::new(content), bounds: Rect::ZERO, path }
  }

  pub fn visible(&self) -> bool { !self.bounds.is_zero_area() }

  pub fn animated(&self) -> bool { false }

  pub fn layout(&mut self, layout: &mut Layout) {
    if let Some(bounds) = self.content.layout(layout) {
      let current = layout.current_bounds();
      self.bounds = current.with_origin(current.origin() + bounds.to_vec2());
    } else {
      self.bounds = layout.current_bounds();
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    if !self.visible() {
      return;
    }

    render.clipped(self.bounds, |render| self.content.draw(render));
  }
}
