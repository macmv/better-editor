use std::{
  any::Any,
  collections::{HashMap, HashSet},
  fmt,
  ops::{Deref, DerefMut},
};

use kurbo::{Point, Rect, Size, Vec2};

mod border;
mod button;
mod padding;
mod stack;

pub use button::Button;
pub use stack::{Align, Justify, Stack};

use crate::{CursorKind, Layout, MouseEvent, Render, WidgetId, WidgetPath};

pub struct WidgetStore {
  pub content: Box<dyn Widget>,
  /// Bounds of this widget, relative to the parent.
  pub bounds:  Rect,
  pub path:    WidgetPath,

  pub visible: bool,
}

pub struct WidgetCollection {
  next_widget_id:     WidgetId,
  pub(crate) paths:   HashMap<WidgetPath, WidgetId>,
  pub(crate) widgets: HashMap<WidgetId, WidgetStore>,

  pub(crate) root: Option<WidgetId>,
  hover_path:      Vec<WidgetId>,
  bounds:          Rect,
}

pub struct WidgetLayout<'a, 'b> {
  layout:  &'a mut Layout<'b>,
  widgets: &'a mut WidgetCollection,

  seen:    HashSet<WidgetId>,
  next_id: u32,
}

pub struct LayoutCtx<'a, 'b> {
  layout:  &'a mut Layout<'b>,
  widgets: &'a mut WidgetCollection,
  stack:   Vec<Rect>,
}

pub struct WidgetMut<'a, W: Widget> {
  pub id:     WidgetId,
  pub widget: &'a mut W,
}

impl fmt::Debug for WidgetCollection {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    writeln!(f, "WidgetCollection {{")?;
    match self.root {
      None => {
        writeln!(f, "  root: None")?;
      }
      Some(root) => {
        writeln!(f, "  root: {:?}", root)?;
        writeln!(f, "  tree:")?;

        let mut stack = vec![(root, 2usize)];
        while let Some((id, depth)) = stack.pop() {
          let indent = "  ".repeat(depth);
          match self.widgets.get(&id) {
            Some(store) => {
              writeln!(
                f,
                "{}- {:?} path: {:?} type: {} bounds: {:?}",
                indent,
                id,
                store.path,
                store.content.type_name(),
                store.bounds
              )?;
              for &child in store.children().iter().rev() {
                stack.push((child, depth + 1));
              }
            }
            None => {
              writeln!(f, "{}- {:?} path: <missing>", indent, id)?;
            }
          }
        }
      }
    }
    writeln!(f, "}}")
  }
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

pub trait Widget: std::any::Any {
  fn layout(&mut self, layout: &mut LayoutCtx) -> Option<Size> {
    let _ = layout;
    None
  }

  fn children(&self) -> &[WidgetId] { &[] }

  fn draw(&mut self, render: &mut Render) { let _ = render; }

  fn on_mouse(&mut self, mouse: &MouseEvent) { let _ = mouse; }
  /// Called when the widget becomes visible or invisible.
  fn on_visible(&mut self, visible: bool) { let _ = visible; }
  /// Called when the widget gains or loses keyboard focus.
  fn on_focus(&mut self, focus: bool) { let _ = focus; }

  fn type_name(&self) -> &'static str { std::any::type_name::<Self>() }

  fn apply_if<U: Widget + 'static>(self, cond: bool, f: impl FnOnce(Self) -> U) -> Box<dyn Widget>
  where
    Self: Sized + 'static,
  {
    if cond { Box::new(f(self)) } else { Box::new(self) }
  }
}

impl WidgetStore {
  pub fn new(path: WidgetPath, content: impl Widget + 'static) -> Self {
    WidgetStore { content: Box::new(content), bounds: Rect::ZERO, path, visible: true }
  }

  pub fn hide(&mut self) { self.visible = false }
  pub fn show(&mut self) { self.visible = true }

  pub fn children(&self) -> &[WidgetId] { self.content.children() }

  pub fn animated(&self) -> bool { false }

  pub fn layout(&mut self, layout: &mut LayoutCtx) -> Size {
    if let Some(size) = self.content.layout(layout) {
      let current = layout.current_bounds();
      self.bounds = current.with_size(size);
    } else {
      self.bounds = layout.current_bounds();
    }
    self.bounds.size()
  }

  pub fn cast<W: Widget>(&self) -> Option<&W> {
    (&*self.content as &dyn std::any::Any).downcast_ref()
  }
  pub fn cast_mut<W: Widget>(&mut self) -> Option<&mut W> {
    (&mut *self.content as &mut dyn std::any::Any).downcast_mut()
  }
}

#[allow(unused)]
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

impl WidgetCollection {
  pub fn new() -> Self {
    WidgetCollection {
      next_widget_id: WidgetId(0),
      paths:          HashMap::new(),
      widgets:        HashMap::new(),
      root:           None,
      hover_path:     Vec::new(),
      bounds:         Rect::ZERO,
    }
  }

  pub fn get(&self, id: WidgetId) -> Option<&WidgetStore> { self.widgets.get(&id) }
  pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut WidgetStore> { self.widgets.get_mut(&id) }

  pub(crate) fn remove(&mut self, id: WidgetId) -> Option<WidgetStore> { self.widgets.remove(&id) }
  pub(crate) fn insert(&mut self, id: WidgetId, store: WidgetStore) {
    self.widgets.insert(id, store);
  }

  pub fn get_path(&self, path: &WidgetPath) -> Option<WidgetId> { self.paths.get(path).copied() }

  pub fn create(&mut self, store: WidgetStore) -> WidgetId {
    let id = self.next_widget_id;
    self.next_widget_id.0 += 1;
    self.paths.insert(store.path.clone(), id);
    self.widgets.insert(id, store);
    id
  }

  pub(crate) fn on_mouse(&mut self, ev: MouseEvent, size: Size, _scale: f64) -> CursorKind {
    match ev {
      MouseEvent::Move { pos } => {
        let Some(new_path) = self.hit_widgets(pos, size) else { return CursorKind::Default };

        self.hover_path(new_path);

        for w in self.hover_path.iter().rev() {
          if let Some(w) = self.widgets.get_mut(w) {
            w.content.on_mouse(&ev);
          }
        }
      }
      MouseEvent::Enter => {}
      MouseEvent::Leave => {
        self.hover_path(vec![]);
      }
      MouseEvent::Button { pos, .. } | MouseEvent::Scroll { pos, .. } => {
        let Some(path) = self.hit_widgets(pos, size) else { return CursorKind::Default };

        for w in path.iter().rev() {
          self.widgets.get_mut(w).unwrap().content.on_mouse(&ev);
        }
      }
    }

    CursorKind::Default
  }

  fn hover_path(&mut self, path: Vec<WidgetId>) {
    if path != self.hover_path {
      let diverge_idx = path
        .iter()
        .zip(self.hover_path.iter())
        .position(|(a, b)| a != b)
        .unwrap_or(path.len().min(self.hover_path.len()));

      for w in self.hover_path[diverge_idx..].iter().rev() {
        if let Some(w) = self.widgets.get_mut(w) {
          w.content.on_mouse(&MouseEvent::Leave);
        }
      }
      for w in path[diverge_idx..].iter().rev() {
        self.widgets.get_mut(w).unwrap().content.on_mouse(&MouseEvent::Enter);
      }

      self.hover_path = path;
    }
  }

  /// Returns a list of all widgets hit by the given point. Parents are returned
  /// first.
  fn hit_widgets(&self, pos: Point, size: Size) -> Option<Vec<WidgetId>> {
    if !self.bounds.contains(pos) {
      return None;
    }
    let pos = pos - self.bounds.origin().to_vec2();

    let mut path = vec![];

    if let Some(root) = self.root {
      let mut stack = vec![(root, Rect::from_origin_size(Point::ZERO, size))];

      while let Some((id, outer_bounds)) = stack.pop() {
        let widget = self.widgets.get(&id).unwrap();
        if !widget.visible {
          continue;
        }

        let bounds = widget.bounds + outer_bounds.origin().to_vec2();
        if bounds.contains(pos) {
          path.push(id);
          stack.extend(widget.children().iter().map(|&c| (c, bounds)));
        }
      }
    }

    Some(path)
  }

  pub fn begin<'a, 'b>(&'a mut self, layout: &'a mut Layout<'b>) -> WidgetLayout<'a, 'b> {
    self.bounds = layout.current_bounds();
    WidgetLayout { layout, widgets: self, seen: HashSet::new(), next_id: 0 }
  }

  pub fn draw(&mut self, render: &mut Render) {
    if let Some(root) = self.root {
      let mut stack = vec![(root, Rect::from_origin_size(Point::ZERO, render.size()))];

      while let Some((id, outer_bounds)) = stack.pop() {
        let widget = self.widgets.get_mut(&id).unwrap();
        if !widget.visible {
          continue;
        }

        let bounds = widget.bounds + outer_bounds.origin().to_vec2();
        render.clipped(bounds, |render| {
          let mut widget = self.widgets.remove(&id).unwrap();
          widget.content.draw(render);
          self.widgets.insert(id, widget);
        });
        let widget = self.widgets.get(&id).unwrap();
        stack.extend(widget.children().iter().rev().map(|&c| (c, bounds)));
      }
    }
  }
}

impl WidgetLayout<'_, '_> {
  pub fn add_widget<'b, W: Widget + 'static>(&'b mut self, widget: W) -> WidgetMut<'b, W> {
    let path = self.next_path();
    self.add_widget_with_path(path, widget)
  }

  fn next_path(&mut self) -> WidgetPath {
    let id = self.next_id;
    self.next_id += 1;
    WidgetPath(vec![id])
  }

  fn add_widget_with_path<'b, W: Widget + 'static>(
    &'b mut self,
    path: WidgetPath,
    widget: W,
  ) -> WidgetMut<'b, W> {
    let id = if let Some(id) = self.clean_widget_at::<W>(&path) {
      id
    } else {
      self.widgets.create(WidgetStore::new(path, widget))
    };
    if !self.seen.insert(id) {
      eprintln!("duplicate widget at path {:?}", self.widgets.get(id).unwrap().path);
    }
    let widget = self.widgets.get_mut(id).unwrap();
    WidgetMut { id, widget: (&mut *widget.content as &mut dyn Any).downcast_mut().unwrap() }
  }

  fn clean_widget_at<W: Widget + 'static>(&self, path: &WidgetPath) -> Option<WidgetId> {
    if let Some(id) = self.widgets.get_path(&path) {
      if (&*self.widgets.get(id).unwrap().content as &dyn Any).type_id()
        != std::any::TypeId::of::<W>()
      {
        return None;
      }

      for child in self.widgets.get(id).unwrap().children() {
        if !self.seen.contains(child) {
          return None;
        }
      }

      Some(id)
    } else {
      None
    }
  }

  pub fn finish(self, root: WidgetId) {
    self.widgets.root = Some(root);

    let mut widget = self.widgets.remove(root).unwrap();
    widget.layout(&mut LayoutCtx { layout: self.layout, widgets: self.widgets, stack: vec![] });
    self.widgets.insert(root, widget);

    // TODO: Tree walk from the root instead of the seen set.
    self.widgets.widgets.retain(|id, widget| {
      if !self.seen.contains(id) {
        self.widgets.paths.remove(&widget.path);
        false
      } else {
        true
      }
    });
  }
}

impl LayoutCtx<'_, '_> {
  pub fn set_bounds(&mut self, child: WidgetId, bounds: Rect) {
    self.widgets.get_mut(child).unwrap().bounds = bounds;
  }

  pub fn size(&self) -> Size {
    if let Some(top) = self.stack.last() { top.size() } else { self.layout.size() }
  }

  fn offset(&self) -> Vec2 {
    if let Some(top) = self.stack.last() { top.origin().to_vec2() } else { Vec2::ZERO }
  }

  pub fn current_bounds(&self) -> Rect {
    Rect::from_origin_size(self.offset().to_point(), self.size())
  }

  pub fn layout_widget(&mut self, root: WidgetId) -> Size {
    let mut widget = self.widgets.remove(root).unwrap();
    let size = widget.layout(self);
    self.widgets.insert(root, widget);
    size
  }
}

impl<'a> Deref for LayoutCtx<'_, 'a> {
  type Target = Layout<'a>;

  fn deref(&self) -> &Self::Target { self.layout }
}

impl DerefMut for LayoutCtx<'_, '_> {
  fn deref_mut(&mut self) -> &mut Self::Target { self.layout }
}

impl<W: Widget> Deref for WidgetMut<'_, W> {
  type Target = W;

  fn deref(&self) -> &Self::Target { self.widget }
}

impl<W: Widget> DerefMut for WidgetMut<'_, W> {
  fn deref_mut(&mut self) -> &mut Self::Target { self.widget }
}
