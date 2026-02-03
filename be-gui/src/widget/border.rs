use kurbo::{Rect, RoundedRect, Size, Stroke};

use crate::{
  Widget, WidgetId,
  widget::{Borders, Corners},
};

pub struct Border {
  borders: Borders,
  radius:  Corners,

  inner: WidgetId,
}

impl Border {
  pub fn new(borders: Borders, inner: WidgetId) -> Self {
    Border { borders, radius: Corners::all(0.0), inner }
  }

  pub fn radius(mut self, radius: f64) -> Self {
    self.radius = Corners::all(radius);
    self
  }
}

impl Widget for Border {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    let size = layout.layout(self.inner);
    layout.set_bounds(
      self.inner,
      Rect::new(
        self.borders.left,
        self.borders.top,
        self.borders.left + size.width,
        self.borders.top + size.height,
      ),
    );

    Some(Size::new(
      self.borders.left + size.width + self.borders.right,
      self.borders.top + size.height + self.borders.bottom,
    ))
  }

  fn children(&self) -> &[crate::WidgetId] { std::slice::from_ref(&self.inner) }

  fn draw(&mut self, render: &mut crate::Render) {
    if self.radius.top_left > 0.0 {
      render.stroke(
        &RoundedRect::from_rect(
          Rect::from_origin_size((0.0, 0.0), render.size()).inset(-self.borders.left),
          self.radius.top_left,
        ),
        render.theme().text,
        Stroke::new(self.borders.left),
      );
      return;
    }

    if self.borders.left > 0.0 {
      render
        .fill(&Rect::new(0.0, 0.0, self.borders.left, render.size().height), render.theme().text);
    }
    if self.borders.right > 0.0 {
      render.fill(
        &Rect::new(
          render.size().width - self.borders.right,
          0.0,
          render.size().width,
          render.size().height,
        ),
        render.theme().text,
      )
    }
  }
}
