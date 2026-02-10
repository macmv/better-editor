use kurbo::Axis;

use crate::{Distance, Widget, WidgetId};

pub struct Split {
  axis:     Axis,
  distance: Distance,
  children: [WidgetId; 2],
}

impl Split {
  pub fn new(axis: Axis, distance: Distance, left: WidgetId, right: WidgetId) -> Self {
    Split { axis, distance, children: [left, right] }
  }
}

impl Widget for Split {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    layout.split(
      &mut self.children,
      self.axis,
      self.distance,
      |children, layout| {
        layout.layout_widget(children[0]);
      },
      |children, layout| {
        layout.layout_widget(children[1]);
      },
    );

    Some(layout.current_bounds().size())
  }

  fn children(&self) -> &[WidgetId] { &self.children }
}
