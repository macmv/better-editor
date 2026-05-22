use kurbo::Point;

use crate::{Font, TextLayout, Widget, widget::LayoutCtx};

pub struct Text {
  content: String,
}

#[derive(Default)]
pub struct TextState {
  layout: Option<TextLayout>,
}

impl Text {
  pub fn new(content: &str) -> Self { Text { content: content.into() } }
}

impl Widget for Text {
  type State = TextState;

  fn layout(&self, state: &mut TextState, layout: &mut LayoutCtx) -> Option<kurbo::Size> {
    if state.layout.as_ref().is_none_or(|l| layout.is_stale(l)) {
      state.layout = Some(layout.layout.layout_text(Font::Ui, &self.content, layout.theme().text));
    }

    Some(state.layout.as_ref().unwrap().size().to_vec2().to_size())
  }

  fn draw(&self, state: &TextState, render: &mut crate::Render) {
    if let Some(layout) = &state.layout {
      render.draw_text(layout, Point::ZERO);
    }
  }
}
