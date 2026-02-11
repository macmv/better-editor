use kurbo::{Rect, Size};

use crate::{Widget, WidgetId, widget::Borders};

#[allow(unused)]
pub struct Padding {
  borders: Borders,

  inner: WidgetId,
}

#[allow(unused)]
impl Padding {
  pub fn new(borders: Borders, inner: WidgetId) -> Self { Padding { borders, inner } }
}

impl Widget for Padding {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    let size = layout.layout_widget(self.inner);
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
}
