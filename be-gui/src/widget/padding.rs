use kurbo::Rect;

use crate::Widget;

pub struct Padding {
  left:   f64,
  top:    f64,
  right:  f64,
  bottom: f64,

  inner: Box<dyn Widget>,
}

impl Padding {
  pub fn new(left: f64, top: f64, right: f64, bottom: f64, inner: impl Widget + 'static) -> Self {
    Padding { left, top, right, bottom, inner: Box::new(inner) }
  }
}

impl Widget for Padding {
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
  }
}
