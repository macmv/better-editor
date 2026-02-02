use kurbo::{Affine, BezPath, Point, Stroke};
use std::sync::LazyLock;

use crate::{Color, Render};

pub enum Icon {
  Stroke(BezPath),
  Fill(BezPath),
}

macro_rules! icon {
  ($(
    $name:ident => $build:tt [$($build_args:tt)*];
  )*) => {
    $(
      pub const $name: LazyLock<Icon> = LazyLock::new(|| build_icon!($build [$($build_args)*]));
    )*
  };
}

macro_rules! build_icon {
  (stroke [$start_point:expr, $($points:expr),* $(,)?]) => {{
    let mut path = BezPath::new();
    path.move_to((Point::from($start_point).to_vec2() / 12.0).to_point());
    $(
      path.line_to((Point::from($points).to_vec2() / 12.0).to_point());
    )*
    Icon::Stroke(path)
  }}
}

icon! {
  CHEVRON_DOWN => stroke [(0.0, 3.0), (6.0, 9.0), (12.0, 3.0)];
  CHEVRON_RIGHT => stroke [(3.0, 0.0), (9.0, 6.0), (3.0, 12.0)];
}

impl Icon {
  pub fn draw(&self, pos: Point, size: f64, color: Color, render: &mut Render) {
    let transform = Affine::translate(pos.to_vec2()) * Affine::scale(size);

    match self {
      Icon::Stroke(path) => {
        render.stroke_transform(path, transform, color, Stroke::new(1.0 / size))
      }
      Icon::Fill(path) => render.fill_transform(path, transform, color),
    }
  }
}
