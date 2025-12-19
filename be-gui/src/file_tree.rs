use std::path::PathBuf;

use kurbo::Rect;

use crate::{Render, theme::Theme};

pub struct FileTree {
  root: PathBuf,
}

impl FileTree {
  pub fn new() -> Self { FileTree { root: PathBuf::new() } }
}

impl FileTree {
  pub fn draw(&self, render: &mut Render) {
    let theme = Theme::current();

    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      theme.background_lower,
    );
  }
}
