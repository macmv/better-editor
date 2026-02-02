use kurbo::{Rect, Size};

mod border;
mod button;

pub use border::Border;
pub use button::Button;

use smol_str::SmolStr;

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

  pub fn animated(&self) -> bool { false }

  pub fn layout(&mut self, layout: &mut Layout) {
    if let Some(size) = self.content.layout(layout) {
      let current = layout.current_bounds();
      self.bounds = current.with_size(size);
    } else {
      self.bounds = layout.current_bounds();
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    render.clipped(self.bounds, |render| self.content.draw(render));
  }

  pub(crate) fn name(&self) -> &SmolStr { self.path.0.last().unwrap() }
}
