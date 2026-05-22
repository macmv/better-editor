use kurbo::{Axis, Rect, Size};

use crate::{Widget, WidgetId, widget::LayoutCtx};

pub struct Stack {
  axis:    Axis,
  align:   Align,
  justify: Justify,
  gap:     f64,

  main_size:  Option<f64>,
  cross_size: Option<f64>,

  children: Vec<WidgetId>,
  sizes:    Vec<Rect>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Align {
  Start,
  Center,
  End,

  SpaceBetween,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Justify {
  Start,
  Center,
  End,
}

impl Stack {
  pub fn new(axis: Axis, align: Align, justify: Justify, children: Vec<WidgetId>) -> Self {
    Stack {
      axis,
      align,
      justify,
      children,
      gap: 0.0,
      main_size: None,
      cross_size: None,
      sizes: vec![],
    }
  }

  pub fn gap(mut self, gap: f64) -> Self {
    self.gap = gap;
    self
  }

  #[allow(dead_code)]
  pub fn main_size(mut self, main_size: f64) -> Self {
    self.main_size = Some(main_size);
    self
  }

  pub fn cross_size(mut self, cross_size: f64) -> Self {
    self.cross_size = Some(cross_size);
    self
  }
}

impl Widget for Stack {
  fn layout(&mut self, layout: &mut LayoutCtx) -> Option<kurbo::Size> {
    self.sizes.clear();
    let mut size = Size::new(0.0, 0.0);
    for child in self.children.iter() {
      let child_size = layout.layout_widget(*child);
      match self.axis {
        Axis::Horizontal => {
          self.sizes.push(Rect::from_origin_size((size.width, 0.0), child_size));
          size.width += child_size.width;
          size.height = child_size.height.max(size.height);
        }
        Axis::Vertical => {
          self.sizes.push(Rect::from_origin_size((0.0, size.height), child_size));
          size.height += child_size.height;
          size.width = child_size.width.max(size.width);
        }
      }
    }

    match self.axis {
      Axis::Horizontal => {
        size.width = self.main_size.unwrap_or(size.width);
        size.height = self.cross_size.unwrap_or(size.height);
      }
      Axis::Vertical => {
        size.height = self.main_size.unwrap_or(size.height);
        size.width = self.cross_size.unwrap_or(size.width);
      }
    }

    let mut main = match (self.axis, self.align) {
      (_, Align::Start | Align::SpaceBetween) => 0.0,
      (Axis::Horizontal, Align::Center) => (layout.current_bounds().width() - size.width) / 2.0,
      (Axis::Horizontal, Align::End) => layout.current_bounds().width() - size.width,
      (Axis::Vertical, Align::Center) => (layout.current_bounds().height() - size.height) / 2.0,
      (Axis::Vertical, Align::End) => layout.current_bounds().height() - size.height,
    };
    let gap = if self.align == Align::SpaceBetween {
      let remaining_space = match self.axis {
        Axis::Horizontal => layout.current_bounds().width() - size.width,
        Axis::Vertical => layout.current_bounds().height() - size.height,
      };
      match self.axis {
        Axis::Horizontal => size.width = layout.current_bounds().width(),
        Axis::Vertical => size.height = layout.current_bounds().height(),
      }
      remaining_space / (self.children.len() - 1) as f64
    } else {
      match self.axis {
        Axis::Horizontal => size.width += self.gap * (self.children.len() - 1) as f64,
        Axis::Vertical => size.height += self.gap * (self.children.len() - 1) as f64,
      }
      self.gap
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
            Justify::Center => (size.height - cross_width) / 2.0,
            Justify::End => size.height - cross_width,
          };
          bounds.y1 = bounds.y0 + cross_width;
          main += main_width + gap;
        }
        Axis::Vertical => {
          let main_width = bounds.height();
          let cross_width = bounds.width();
          bounds.y0 = main;
          bounds.y1 = main + main_width;
          bounds.x0 = match self.justify {
            Justify::Start => 0.0,
            Justify::Center => (size.width - cross_width) / 2.0,
            Justify::End => size.width - cross_width,
          };
          bounds.x1 = bounds.x0 + cross_width;
          main += main_width + gap;
        }
      };

      layout.set_bounds(child, *bounds);
    }

    if let Some(main_size) = self.main_size {
      match self.axis {
        Axis::Horizontal => size.width = main_size,
        Axis::Vertical => size.height = main_size,
      }
    }
    if let Some(cross_size) = self.cross_size {
      match self.axis {
        Axis::Horizontal => size.height = cross_size,
        Axis::Vertical => size.width = cross_size,
      }
    }

    Some(size)
  }

  fn children(&self) -> &[WidgetId] { &self.children }
}
