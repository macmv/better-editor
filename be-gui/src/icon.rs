use kurbo::{Affine, BezPath, Point, Stroke};

use crate::{Color, Render};

// See `be-lucide-importer` for details. It basically just finds these comments
// and imports the according icons.
//
// !!ICON IMPORT START!!
// - devicons::git
// - devicons::markdown
// - devicons::rust
// - lucide::chevron-down
// - lucide::chevron-right
// - lucide::folder
// - lucide::minus
// - lucide::square-dot
// - lucide::square-plus
// - lucide::square-slash
// - lucide::text-align-start
// !!ICON IMPORT END!!
mod generated;

pub use generated::*;

pub struct Icon {
  path: BezPath,
  size: f64,
}

impl Icon {
  pub fn stroke(&self, pos: Point, size: f64, color: Color, render: &mut Render) {
    let transform = Affine::translate(pos.to_vec2()) * Affine::scale(size / self.size);

    render.stroke_transform(&self.path, transform, color, Stroke::new(2.0));
  }

  pub fn fill(&self, pos: Point, size: f64, color: Color, render: &mut Render) {
    let transform = Affine::translate(pos.to_vec2()) * Affine::scale(size / self.size);

    render.fill_transform(&self.path, transform, color);
  }
}
