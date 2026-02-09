use kurbo::{Rect, Size};

mod border;
mod button;
mod padding;
mod stack;

pub use border::Border;
pub use button::Button;
pub use padding::Padding;
pub use stack::{Align, Justify, Stack};

use crate::{Layout, MouseEvent, Render, WidgetId, WidgetPath};

pub struct WidgetStore {
  pub content: Box<dyn Widget>,
  /// Bounds of this widget, relative to the parent.
  pub bounds:  Rect,
  pub path:    WidgetPath,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Borders {
  pub left:   f64,
  pub top:    f64,
  pub right:  f64,
  pub bottom: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Corners {
  pub top_left:     f64,
  pub top_right:    f64,
  pub bottom_left:  f64,
  pub bottom_right: f64,
}

macro_rules! op {
  ($name:ident($($arg_name:ident: $arg_ty:ty),*) -> $ty:ident::$new:ident($($arg_expr:expr),*)) => {
    pub fn $name(self, $($arg_name: $arg_ty),*) -> WidgetBuilder<'a, 'b, $ty> {
      self.wrap(|id| crate::widget::$ty::$new($($arg_expr),*, id))
    }
  }
}

pub trait Widget: std::any::Any {
  fn layout(&mut self, layout: &mut Layout) -> Option<Size> {
    let _ = layout;
    None
  }

  fn children(&self) -> &[WidgetId] { &[] }

  fn draw(&mut self, render: &mut Render) { let _ = render; }

  fn on_mouse(&mut self, mouse: &MouseEvent) { let _ = mouse; }

  fn apply_if<U: Widget + 'static>(self, cond: bool, f: impl FnOnce(Self) -> U) -> Box<dyn Widget>
  where
    Self: Sized + 'static,
  {
    if cond { Box::new(f(self)) } else { Box::new(self) }
  }
}

impl Widget for Box<dyn Widget> {
  fn layout(&mut self, layout: &mut Layout) -> Option<Size> { (**self).layout(layout) }
  fn children(&self) -> &[WidgetId] { (**self).children() }
  fn draw(&mut self, render: &mut Render) { (**self).draw(render) }
  fn on_mouse(&mut self, mouse: &MouseEvent) { (**self).on_mouse(mouse); }
}

impl WidgetStore {
  pub fn new(path: WidgetPath, content: impl Widget + 'static) -> Self {
    WidgetStore { content: Box::new(content), bounds: Rect::ZERO, path }
  }

  pub fn children(&self) -> &[WidgetId] { self.content.children() }

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

impl From<f64> for Borders {
  fn from(b: f64) -> Self { Borders::all(b) }
}

impl Corners {
  pub const fn all(c: f64) -> Self {
    Corners { top_left: c, top_right: c, bottom_left: c, bottom_right: c }
  }
}

impl From<f64> for Corners {
  fn from(c: f64) -> Self { Corners::all(c) }
}

/*
impl<'a, 'b, W: Widget> WidgetBuilder<'a, 'b, W> {
  op!(padding(p: impl Into<Borders>) -> Padding::new(p.into()));
  op!(padding_left(left: f64) -> Padding::new(Borders::left(left)));
  op!(padding_top(top: f64) -> Padding::new(Borders::top(top)));
  op!(padding_right(right: f64) -> Padding::new(Borders::right(right)));
  op!(padding_bottom(bottom: f64) -> Padding::new(Borders::bottom(bottom)));
  op!(padding_left_right(p: f64) -> Padding::new(Borders::left_right(p)));
  op!(padding_top_bottom(p: f64) -> Padding::new(Borders::top_bottom(p)));
}
*/
