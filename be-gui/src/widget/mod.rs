use kurbo::{Rect, Size};

mod border;
mod button;
mod padding;
mod stack;

pub use border::Border;
pub use button::Button;
pub use padding::Padding;
pub use stack::{Align, Justify, Stack};

use crate::{Layout, Render, WidgetPath};

pub struct WidgetStore {
  pub content: Box<dyn Widget>,
  pub bounds:  Rect,
  pub path:    WidgetPath,
}

pub struct Borders {
  pub left:   f64,
  pub top:    f64,
  pub right:  f64,
  pub bottom: f64,
}

#[allow(dead_code)]
pub struct Corners {
  pub top_left:     f64,
  pub top_right:    f64,
  pub bottom_left:  f64,
  pub bottom_right: f64,
}

macro_rules! op {
  ($name:ident($($arg_name:ident: $arg_ty:ty),*) -> $ty:ident::new($($arg_expr:expr),*)) => {
    fn $name(self, $($arg_name: $arg_ty),*) -> $ty
    where
      Self: Sized + 'static,
    {
      $ty::new($($arg_expr),*, self)
    }
  }
}

pub trait Widget {
  fn layout(&mut self, layout: &mut Layout) -> Option<Size> {
    let _ = layout;
    None
  }

  fn draw(&mut self, render: &mut Render) { let _ = render; }

  fn apply_if<U: Widget + 'static>(self, cond: bool, f: impl FnOnce(Self) -> U) -> Box<dyn Widget>
  where
    Self: Sized + 'static,
  {
    if cond { Box::new(f(self)) } else { Box::new(self) }
  }

  op!(border(b: f64) -> Border::new(Borders::all(b)));
  op!(border_left(left: f64) -> Border::new(Borders::left(left)));
  op!(border_top(top: f64) -> Border::new(Borders::top(top)));
  op!(border_right(right: f64) -> Border::new(Borders::right(right)));
  op!(border_bottom(bottom: f64) -> Border::new(Borders::bottom(bottom)));
  op!(border_left_right(b: f64) -> Border::new(Borders::left_right(b)));
  op!(border_top_bottom(b: f64) -> Border::new(Borders::top_bottom(b)));

  op!(padding(p: f64) -> Padding::new(p, p, p, p));
  op!(padding_left(left: f64) -> Padding::new(left, 0.0, 0.0, 0.0));
  op!(padding_top(top: f64) -> Padding::new(0.0, top, 0.0, 0.0));
  op!(padding_right(right: f64) -> Padding::new(0.0, 0.0, right, 0.0));
  op!(padding_bottom(bottom: f64) -> Padding::new(0.0, 0.0, 0.0, bottom));
  op!(padding_left_right(p: f64) -> Padding::new(p, 0.0, p, 0.0));
  op!(padding_top_bottom(p: f64) -> Padding::new(0.0, p, 0.0, p));
}

impl Widget for Box<dyn Widget> {
  fn layout(&mut self, layout: &mut Layout) -> Option<Size> { (**self).layout(layout) }
  fn draw(&mut self, render: &mut Render) { (**self).draw(render) }
}

impl WidgetStore {
  pub fn new(path: WidgetPath, content: impl Widget + 'static) -> Self {
    WidgetStore { content: Box::new(content), bounds: Rect::ZERO, path }
  }

  pub fn animated(&self) -> bool { false }

  pub fn layout(&mut self, layout: &mut Layout) -> Size {
    if let Some(size) = self.content.layout(layout) {
      let current = layout.current_bounds();
      self.bounds = current.with_size(size);
    } else {
      self.bounds = layout.current_bounds();
    }
    self.bounds.size()
  }

  pub fn draw(&mut self, render: &mut Render) {
    render.clipped(self.bounds, |render| self.content.draw(render));
  }
}

impl Borders {
  pub const fn all(b: f64) -> Self { Borders { left: b, top: b, right: b, bottom: b } }

  pub const fn left(left: f64) -> Self { Borders { left, right: 0.0, top: 0.0, bottom: 0.0 } }
  pub const fn right(right: f64) -> Self { Borders { left: 0.0, right, top: 0.0, bottom: 0.0 } }
  pub const fn top(top: f64) -> Self { Borders { left: 0.0, right: 0.0, top, bottom: 0.0 } }
  pub const fn bottom(bottom: f64) -> Self { Borders { left: 0.0, right: 0.0, top: 0.0, bottom } }

  pub const fn left_right(b: f64) -> Self { Borders { left: b, right: b, top: 0.0, bottom: 0.0 } }
  pub const fn top_bottom(b: f64) -> Self { Borders { left: 0.0, right: 0.0, top: b, bottom: b } }
}

impl Corners {
  pub const fn all(c: f64) -> Self {
    Corners { top_left: c, top_right: c, bottom_left: c, bottom_right: c }
  }
}
