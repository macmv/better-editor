use be_input::{Action, Edit};
use kurbo::{Point, Rect, RoundedRect, Stroke};

use crate::Render;

pub struct Search {
  search: String,
  cursor: usize, // in bytes
}

impl Search {
  pub fn new() -> Self { Search { search: String::new(), cursor: 0 } }

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

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Edit { e: Edit::Insert(c), .. } => {
        self.search.push(c);
        self.cursor += c.len_utf8();
      }

      _ => {}
    }
  }
}
