use kurbo::Rect;

use crate::{Render, oklch};

pub struct Editor {}

impl Editor {
  pub fn new() -> Self { Editor {} }

  pub fn draw(&self, render: &mut Render) {
    render
      .fill(&Rect::new(0.0, 0.0, render.size().width, render.size().height), oklch(0.3, 0.0, 0.0));
  }
}
