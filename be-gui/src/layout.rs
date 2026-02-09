use std::{
  any::Any,
  collections::HashSet,
  ops::{Deref, DerefMut},
};

use kurbo::{Axis, Point, Rect, Size, Vec2};

use crate::{
  Color, Distance, Notify, RenderStore, TextLayout, ViewId, Widget, WidgetId, WidgetPath,
  WidgetStore, theme::Theme,
};

pub struct Layout<'a> {
  pub store: &'a mut RenderStore,

  scale: f64,
  size:  Size,

  stack:   Vec<Rect>,
  path:    WidgetPath,
  next_id: u32,

  pub(crate) seen: HashSet<WidgetId>,

  pub active:   Option<ViewId>,
  pub to_close: Vec<ViewId>,
}

pub struct WidgetMut<'a, W: Widget> {
  pub id:     WidgetId,
  pub widget: &'a mut W,
}

impl<'a> Layout<'a> {
  pub fn new(store: &'a mut RenderStore, scale: f64, size: Size) -> Self {
    Self {
      store,
      scale,
      size,
      stack: vec![],
      path: WidgetPath(vec![]),
      next_id: 0,
      seen: HashSet::new(),
      active: None,
      to_close: vec![],
    }
  }

  pub fn size(&self) -> Size {
    if let Some(top) = self.stack.last() { top.size() } else { self.size }
  }

  fn offset(&self) -> Vec2 {
    if let Some(top) = self.stack.last() { top.origin().to_vec2() } else { Vec2::ZERO }
  }

  pub fn notifier(&self) -> Notify { self.store.notifier() }
  pub fn theme(&self) -> &Theme { &self.store.theme }

  pub fn current_bounds(&self) -> Rect {
    Rect::from_origin_size(self.offset().to_point(), self.size())
  }

  pub fn add_widget<'b, W: Widget + 'static>(
    &'b mut self,
    widget: impl FnOnce() -> W,
  ) -> WidgetMut<'b, W> {
    let path = self.next_path();

    let id = if let Some(id) = self.store.widgets.get_path(&path) {
      id
    } else {
      self.store.widgets.create(WidgetStore::new(path, widget()))
    };
    if !self.seen.insert(id) {
      eprintln!("duplicate widget at path {:?}", self.store.widgets.widgets[&id].path);
    }
    let widget = self.store.widgets.widgets.get_mut(&id).unwrap();
    WidgetMut { id, widget: (&mut *widget.content as &mut dyn Any).downcast_mut().unwrap() }
  }

  pub fn layout(&mut self, root: WidgetId) -> Size {
    let mut widget = self.store.widgets.widgets.remove(&root).unwrap();
    let size = widget.layout(self);
    self.store.widgets.widgets.insert(root, widget);
    size
  }

  pub fn set_bounds(&mut self, child: WidgetId, bounds: Rect) {
    self.store.widgets.widgets.get_mut(&child).unwrap().bounds = bounds;
  }

  fn next_path(&mut self) -> WidgetPath {
    let mut path = self.path.clone();
    path.0.push(self.next_id);
    self.next_id += 1;
    path
  }

  pub fn split<S>(
    &mut self,
    state: &mut S,
    axis: Axis,
    distance: Distance,
    left: impl FnOnce(&mut S, &mut Layout),
    right: impl FnOnce(&mut S, &mut Layout),
  ) {
    let mut left_bounds = Rect::from_origin_size(Point::ZERO, self.size());
    let mut right_bounds = Rect::from_origin_size(Point::ZERO, self.size());

    match axis {
      Axis::Vertical => {
        let mut distance = distance.to_pixels_in(self.size().width);
        if distance < 0.0 {
          distance += self.size().width;
        }

        left_bounds.x1 = distance;
        right_bounds.x0 = distance;
      }
      Axis::Horizontal => {
        let mut distance = distance.to_pixels_in(self.size().height);
        if distance < 0.0 {
          distance += self.size().height;
        }

        left_bounds.y1 = distance;
        right_bounds.y0 = distance;
      }
    }

    self.clipped(left_bounds, |render| left(state, render));
    self.clipped(right_bounds, |render| right(state, render));
  }

  pub fn clipped<R>(&mut self, mut rect: Rect, f: impl FnOnce(&mut Layout) -> R) -> R {
    rect = rect + self.offset();

    let scaled_rect = rect.scale_from_origin(self.scale).round();
    self.stack.push(scaled_rect.scale_from_origin(1.0 / self.scale));
    self.path.0.push(self.next_id);
    self.next_id = 0;

    let ret = f(self);

    self.next_id = self.path.0.pop().expect("no clip layer to pop");
    self.stack.pop();

    ret
  }

  pub fn close_view(&mut self) {
    if let Some(id) = self.active {
      self.to_close.push(id);
    } else {
      panic!("no active view set");
    }
  }

  pub fn build_layout(
    &mut self,
    mut layout: parley::Layout<peniko::Brush>,
    backgrounds: Vec<(usize, Option<peniko::Brush>)>,
  ) -> TextLayout {
    layout.break_all_lines(None);
    layout.align(None, parley::Alignment::Start, parley::AlignmentOptions::default());

    TextLayout {
      metrics: self.store.text.font_metrics().clone(),
      layout,
      backgrounds,
      scale: self.scale,
    }
  }

  pub fn is_stale(&self, layout: &TextLayout) -> bool { layout.scale != self.scale }

  pub fn layout_text(&mut self, text: &str, color: Color) -> TextLayout {
    puffin::profile_function!();

    let builder = self.store.text.layout_builder(text, color, self.scale);

    let (built, backgrounds) = builder.build(text);
    self.build_layout(built, backgrounds)
  }
}

impl Drop for Layout<'_> {
  fn drop(&mut self) {
    self.store.widgets.widgets.retain(|id, widget| {
      if !self.seen.contains(id) {
        self.store.widgets.paths.remove(&widget.path);
        false
      } else {
        true
      }
    });
  }
}

impl<W: Widget> Deref for WidgetMut<'_, W> {
  type Target = W;

  fn deref(&self) -> &Self::Target { self.widget }
}

impl<W: Widget> DerefMut for WidgetMut<'_, W> {
  fn deref_mut(&mut self) -> &mut Self::Target { self.widget }
}
