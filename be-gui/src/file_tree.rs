use std::path::{Path, PathBuf};

use kurbo::Rect;

use crate::{Render, theme::Theme};

pub struct FileTree {
  root: PathBuf,

  tree: Directory,
}

enum Item {
  File(File),
  Directory(Directory),
}

struct Directory {
  name:     String,
  items:    Option<Vec<Item>>,
  expanded: bool,
}

struct File {
  name: String,
}

impl FileTree {
  pub fn current_directory() -> Self { FileTree::new(Path::new(".")) }

  pub fn new(path: &Path) -> Self {
    let path = path.canonicalize().unwrap();
    let tree = Directory::new(&path);

    FileTree { root: path, tree }
  }
}

impl Directory {
  fn new(path: &Path) -> Directory {
    Directory {
      name:     path.file_name().unwrap().to_string_lossy().to_string(),
      items:    None,
      expanded: false,
    }
  }
}

impl FileTree {
  pub fn draw(&self, render: &mut Render) {
    let theme = Theme::current();

    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      theme.background_lower,
    );

    self.tree.draw(render);
  }
}

impl Directory {
  fn draw(&self, render: &mut Render) {
    let theme = Theme::current();

    render.fill(&Rect::new(0.0, 0.0, render.size().width, 20.0), theme.background_raised);

    let text = render.layout_text(&self.name, (20.0, 0.0), theme.text);
    render.draw_text(&text);
  }
}
