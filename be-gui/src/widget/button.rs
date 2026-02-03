use kurbo::Point;

use crate::{TextLayout, Widget};

pub struct Button {
  content: String,
  hover:   bool,

  layout: Option<TextLayout>,
}

impl Button {
  pub fn new(content: &str) -> Self {
    Button { content: content.into(), hover: false, layout: None }
  }
}

impl Widget for Button {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    if self.layout.as_ref().is_none_or(|l| layout.is_stale(l)) {
      self.layout = Some(layout.layout_text(&self.content, layout.theme().text));
    }

    Some(self.layout.as_ref().unwrap().size())
  }

  fn draw(&mut self, render: &mut crate::Render) {
    if let Some(layout) = &mut self.layout {
      render.draw_text(layout, Point::ZERO);
    }
  }

  fn on_mouse(&mut self, mouse: &crate::MouseEvent) {
    match mouse {
      crate::MouseEvent::Move { .. } => self.hover = true,
      crate::MouseEvent::Leave { .. } => self.hover = false,

      _ => (),
    }

    dbg!(&self.hover);
  }
}
