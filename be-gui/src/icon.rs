use kurbo::{Affine, BezPath, Point, Stroke};

use crate::{Color, Render};

// See `be-lucide-importer` for details. It basically just finds these comments
// and imports the according icons.
//
// !!ICON IMPORT START!!
// - chevron-down
// - chevron-right
// - folder
// - minus
// - pen
// - plus
// !!ICON IMPORT END!!
mod lucide;

pub use lucide::*;

// All lucide icons are 24x24.
const SIZE_BASE: f64 = 24.0;

pub struct Icon {
  path: BezPath,
}

impl Icon {
  pub fn stroke(&self, pos: Point, size: f64, color: Color, render: &mut Render) {
    let transform = Affine::translate(pos.to_vec2()) * Affine::scale(size / SIZE_BASE);

    render.stroke_transform(&self.path, transform, color, Stroke::new(2.0));
  }

  pub fn fill(&self, pos: Point, size: f64, color: Color, render: &mut Render) {
    let transform = Affine::translate(pos.to_vec2()) * Affine::scale(size / SIZE_BASE);

    render.fill_transform(&self.path, transform, color);
  }
}
