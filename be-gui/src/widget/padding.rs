use kurbo::{Rect, Size};

use crate::{Widget, WidgetId};

pub struct Padding {
  left:   f64,
  top:    f64,
  right:  f64,
  bottom: f64,

  inner: WidgetId,
}

impl Padding {
  pub fn new(left: f64, top: f64, right: f64, bottom: f64, inner: WidgetId) -> Self {
    Padding { left, top, right, bottom, inner }
  }
}

impl Widget for Padding {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    let size = layout.layout(self.inner);
    layout.set_bounds(
      self.inner,
      Rect::new(self.left, self.top, self.left + size.width, self.top + size.height),
    );

    Some(Size::new(self.left + size.width + self.right, self.top + size.height + self.bottom))
  }

  fn children(&self) -> &[crate::WidgetId] { std::slice::from_ref(&self.inner) }
}
