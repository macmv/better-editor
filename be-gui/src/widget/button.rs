use kurbo::{Point, Rect, Vec2};

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

const BORDER_WIDTH: f64 = 1.0;
const BORDER_RADIUS: f64 = 5.0;
const HORIZONTAL_PADDING: f64 = 5.0;

impl Widget for Button {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    if self.layout.as_ref().is_none_or(|l| layout.is_stale(l)) {
      self.layout = Some(layout.layout_text(&self.content, layout.theme().text));
    }

    Some(
      (self.layout.as_ref().unwrap().size().to_vec2()
        + Vec2::splat(BORDER_WIDTH * 2.0)
        + Vec2::new(HORIZONTAL_PADDING * 2.0, 0.0))
      .to_size(),
    )
  }

  fn draw(&mut self, render: &mut crate::Render) {
    if self.hover {
      render.fill(
        &Rect::from_origin_size(Point::ZERO, render.size()),
        render.theme().background_raised_outline,
      );
    }

    if let Some(layout) = &mut self.layout {
      super::Border {
        borders: super::Borders::all(BORDER_WIDTH),
        radius:  super::Corners::all(BORDER_RADIUS),
      }
      .draw(render);

      render.draw_text(layout, Point::new(BORDER_WIDTH + HORIZONTAL_PADDING, BORDER_WIDTH));
    }
  }

  fn on_mouse(&mut self, mouse: &crate::MouseEvent) {
    match mouse {
      crate::MouseEvent::Enter => self.hover = true,
      crate::MouseEvent::Leave => self.hover = false,

      _ => {}
    }
  }
}
