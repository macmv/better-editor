use kurbo::{Axis, Point, Rect, Size};

use crate::{Distance, Widget, WidgetId};

pub struct Split {
  axis:     Axis,
  distance: Distance,
  children: [WidgetId; 2],
}

pub struct ManySplit {
  axis:     Axis,
  percent:  Vec<f64>,
  children: Vec<WidgetId>,
}

impl Split {
  pub fn new(axis: Axis, distance: Distance, left: WidgetId, right: WidgetId) -> Self {
    Split { axis, distance, children: [left, right] }
  }
}

impl ManySplit {
  pub fn new(axis: Axis, percent: Vec<f64>, children: Vec<WidgetId>) -> Self {
    ManySplit { axis, percent, children }
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

impl Widget for ManySplit {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    let mut x = 0.0;
    for (i, child) in self.children.iter().enumerate() {
      let percent =
        self.percent.get(i).copied().unwrap_or_else(|| 1.0 - self.percent.iter().sum::<f64>());
      let pixels = Distance::Percent(percent)
        .to_pixels_in(match self.axis {
          Axis::Vertical => layout.size().width,
          Axis::Horizontal => layout.size().height,
        })
        .round();

      match self.axis {
        Axis::Vertical => {
          layout.clipped(
            Rect::from_origin_size(Point::new(x, 0.0), Size::new(pixels, layout.size().height)),
            |layout| layout.layout_widget(*child),
          );
        }
        Axis::Horizontal => {
          layout.clipped(
            Rect::from_origin_size(Point::new(0.0, x), Size::new(layout.size().width, pixels)),
            |layout| layout.layout_widget(*child),
          );
        }
      }

      x += pixels;
    }

    Some(layout.current_bounds().size())
  }

  fn children(&self) -> &[WidgetId] { &self.children }
}
