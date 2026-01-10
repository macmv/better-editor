use kurbo::{Point, Rect, RoundedRect, Stroke};

use crate::Render;

pub struct Search {}

impl Search {
  pub fn new() -> Self { Search {} }

  pub fn draw(&mut self, render: &mut Render) {
    let bounds = Rect::from_origin_size(Point::ZERO, render.size());

    let radius = 20.0;
    render.fill(&RoundedRect::from_rect(bounds, radius), render.theme().background_raised);
    let stroke = 1.0 / render.scale();
    render.stroke(
      &RoundedRect::from_rect(bounds.inset(-stroke), radius),
      render.theme().background_raised_outline,
      Stroke::new(stroke),
    );
  }
}
