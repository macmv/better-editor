use kurbo::{Rect, RoundedRect, Stroke};

use crate::{
  Widget,
  widget::{Borders, Corners},
};

pub struct Border {
  borders: Borders,
  radius:  Corners,

  inner: Box<dyn Widget>,
}

impl Border {
  pub fn new(borders: Borders, inner: impl Widget + 'static) -> Self {
    Border { borders, radius: Corners::all(0.0), inner: Box::new(inner) }
  }

  pub fn radius(mut self, radius: f64) -> Self {
    self.radius = Corners::all(radius);
    self
  }
}

impl Widget for Border {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    let mut size = self.inner.layout(layout)?;
    size.width += self.borders.left + self.borders.right;
    size.height += self.borders.top + self.borders.bottom;

    Some(size)
  }

  fn draw(&mut self, render: &mut crate::Render) {
    render.clipped(
      Rect::new(
        self.borders.left,
        self.borders.top,
        render.size().width - self.borders.right,
        render.size().height - self.borders.bottom,
      ),
      |render| self.inner.draw(render),
    );

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
