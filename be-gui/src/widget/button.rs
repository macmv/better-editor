use kurbo::{Point, Vec2};

use crate::{Font, TextLayout, Widget, widget::LayoutCtx};

pub struct Button {
  content: String,
  active:  bool,
}

#[derive(Default)]
pub struct ButtonState {
  hover:         bool,
  pending_press: bool,
  layout:        Option<TextLayout>,
}

impl Button {
  pub fn new(content: &str) -> Self { Button { content: content.into(), active: false } }

  /// Highlights the button as the currently selected one.
  pub fn active(mut self, active: bool) -> Self {
    self.active = active;
    self
  }
}

impl ButtonState {
  pub fn pressed(&self) -> bool { self.pending_press }
}

const BORDER_WIDTH: f64 = 1.0;
const BORDER_RADIUS: f64 = 4.0;
const HORIZONTAL_PADDING: f64 = 5.0;

impl Widget for Button {
  type State = ButtonState;

  fn layout(&self, state: &mut ButtonState, layout: &mut LayoutCtx) -> Option<kurbo::Size> {
    state.pending_press = false;

    if state.layout.as_ref().is_none_or(|l| layout.is_stale(l)) {
      state.layout = Some(layout.layout.layout_text(Font::Ui, &self.content, layout.theme().text));
    }

    Some(
      (state.layout.as_ref().unwrap().size().to_vec2()
        + Vec2::splat(BORDER_WIDTH * 2.0)
        + Vec2::new(HORIZONTAL_PADDING * 2.0, 0.0))
      .to_size(),
    )
  }

  fn draw(&self, state: &ButtonState, render: &mut crate::Render) {
    if let Some(layout) = &state.layout {
      let border = super::border::Border {
        borders: super::Borders::all(BORDER_WIDTH),
        radius:  super::Corners::all(BORDER_RADIUS),
      };

      super::border::Border {
        borders: super::Borders::all(0.0),
        radius:  super::Corners::all(BORDER_RADIUS),
      }
      .draw_inside(
        render,
        if self.active {
          render.theme().background_raised_outline
        } else {
          render.theme().background_raised
        },
      );
      if state.hover || self.active {
        border.draw_inside(render, render.theme().background_raised_outline);
      }

      render.draw_text(layout, Point::new(BORDER_WIDTH + HORIZONTAL_PADDING, BORDER_WIDTH));
    }
  }

  fn on_mouse(&self, state: &mut ButtonState, mouse: &crate::MouseEvent) {
    match mouse {
      crate::MouseEvent::Enter => state.hover = true,
      crate::MouseEvent::Leave => state.hover = false,

      crate::MouseEvent::Button { pressed: false, .. } => {
        state.pending_press = true;
      }

      _ => {}
    }
  }
}
