use kurbo::Point;

use crate::{Font, TextLayout, Widget, widget::LayoutCtx};

pub struct Text {
  content: String,

  layout: Option<TextLayout>,
}

impl Text {
  pub fn new(content: &str) -> Self { Text { content: content.into(), layout: None } }
}

impl Widget for Text {
  fn layout(&mut self, layout: &mut LayoutCtx) -> Option<kurbo::Size> {
    if self.layout.as_ref().is_none_or(|l| layout.is_stale(l)) {
      self.layout = Some(layout.layout.layout_text(Font::Ui, &self.content, layout.theme().text));
    }

    Some(self.layout.as_ref().unwrap().size().to_vec2().to_size())
  }

  fn draw(&mut self, render: &mut crate::Render) {
    if let Some(layout) = &mut self.layout {
      render.draw_text(layout, Point::ZERO);
    }
  }
}

