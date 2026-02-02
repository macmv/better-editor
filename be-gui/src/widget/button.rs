use kurbo::Point;

use crate::{TextLayout, Widget};

pub struct Button {
  content: String,

  layout: Option<TextLayout>,
}

impl Widget for Button {
  fn layout(&mut self, layout: &mut crate::Layout) { let _ = layout; }
  fn draw(&mut self, render: &mut crate::Render) {
    if self.layout.is_none() {
      self.layout = Some(render.layout_text(&self.content, render.theme().text));
    }

    if let Some(layout) = &mut self.layout {
      render.draw_text(layout, Point::ZERO);
    }
  }
}
