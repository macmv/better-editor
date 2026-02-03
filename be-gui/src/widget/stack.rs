use kurbo::{Axis, Rect, Size};

use crate::{Widget, WidgetId};

pub struct Stack {
  axis:    Axis,
  align:   Align,
  justify: Justify,
  gap:     f64,

  children: Vec<WidgetId>,
  sizes:    Vec<Rect>,
}

#[derive(Clone, Copy)]
pub enum Align {
  Start,
  Center,
  End,
}

#[derive(Clone, Copy)]
pub enum Justify {
  Start,
  Center,
  End,
}

impl Stack {
  pub fn new(axis: Axis, align: Align, justify: Justify, children: Vec<WidgetId>) -> Self {
    Stack { axis, align, justify, children, gap: 0.0, sizes: vec![] }
  }

  pub fn gap(mut self, gap: f64) -> Self {
    self.gap = gap;
    self
  }
}

impl Widget for Stack {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    self.sizes.clear();
    let mut size = Size::new(0.0, 0.0);
    for child in self.children.iter() {
      let child_size = layout.layout(*child);
      match self.axis {
        Axis::Horizontal => {
          self.sizes.push(Rect::from_origin_size((size.width, 0.0), child_size));
          size.width += child_size.width + self.gap;
          size.height = child_size.height.max(size.height);
        }
        Axis::Vertical => {
          self.sizes.push(Rect::from_origin_size((0.0, size.height), child_size));
          size.height += child_size.height + self.gap;
          size.width = child_size.width.max(size.width);
        }
      }
    }

    let mut main = match (self.axis, self.align) {
      (_, Align::Start) => 0.0,
      (Axis::Horizontal, Align::Center) => (layout.current_bounds().width() - size.width) / 2.0,
      (Axis::Horizontal, Align::End) => layout.current_bounds().width() - size.width,
      (Axis::Vertical, Align::Center) => (layout.current_bounds().height() - size.height) / 2.0,
      (Axis::Vertical, Align::End) => layout.current_bounds().height() - size.height,
    };
    for (&child, bounds) in self.children.iter().zip(self.sizes.iter_mut()) {
      match self.axis {
        Axis::Horizontal => {
          let main_width = bounds.width();
          let cross_width = bounds.height();
          bounds.x0 = main;
          bounds.x1 = main + main_width;
          bounds.y0 = match self.justify {
            Justify::Start => 0.0,
            Justify::Center => (layout.current_bounds().height() - size.height) / 2.0,
            Justify::End => layout.current_bounds().height() - size.height,
          };
          bounds.y1 = bounds.y0 + cross_width;
          main += main_width + self.gap;
        }
        Axis::Vertical => {
          let main_width = bounds.height();
          let cross_width = bounds.width();
          bounds.y0 = main;
          bounds.y1 = main + main_width;
          bounds.x0 = match self.justify {
            Justify::Start => 0.0,
            Justify::Center => (layout.current_bounds().width() - size.width) / 2.0,
            Justify::End => layout.current_bounds().width() - size.width,
          };
          bounds.x1 = bounds.x0 + cross_width;
          main += main_width + self.gap;
        }
      };

      layout.set_bounds(child, *bounds);
    }

    Some(size)
  }
}
