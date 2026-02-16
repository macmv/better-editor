use kurbo::{Axis, Point, Rect, Size, Vec2};

use crate::{Color, Distance, Notify, RenderStore, TextLayout, ViewId, theme::Theme};

pub struct Layout<'a> {
  pub store: &'a mut RenderStore,

  scale: f64,
  size:  Size,

  stack: Vec<Rect>,

  pub active:   Option<ViewId>,
  pub to_close: Vec<ViewId>,
}

impl<'a> Layout<'a> {
  pub fn new(store: &'a mut RenderStore, scale: f64, size: Size) -> Self {
    Self { store, scale, size, stack: vec![], active: None, to_close: vec![] }
  }

  /// TODO: Don't expose this.
  pub(crate) fn scale(&self) -> f64 { self.scale }

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

    let ret = f(self);

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
