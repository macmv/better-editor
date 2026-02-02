use kurbo::Rect;

use crate::Widget;

pub struct Border {
  left:   f64,
  top:    f64,
  right:  f64,
  bottom: f64,

  inner: Box<dyn Widget>,
}

impl Border {
  pub fn new(left: f64, top: f64, right: f64, bottom: f64, inner: impl Widget + 'static) -> Self {
    Border { left, top, right, bottom, inner: Box::new(inner) }
  }
}

impl Widget for Border {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    let mut size = self.inner.layout(layout)?;
    size.width += self.left + self.right;
    size.height += self.top + self.bottom;

    Some(size)
  }

  fn draw(&mut self, render: &mut crate::Render) {
    render.clipped(
      Rect::new(
        self.left,
        self.top,
        render.size().width - self.right,
        render.size().height - self.bottom,
      ),
      |render| self.inner.draw(render),
    );

    if self.left > 0.0 {
      render.fill(&Rect::new(0.0, 0.0, self.left, render.size().height), render.theme().text);
    }
    if self.right > 0.0 {
      render.fill(
        &Rect::new(
          render.size().width - self.right,
          0.0,
          render.size().width,
          render.size().height,
        ),
        render.theme().text,
      )
    }
  }
}
