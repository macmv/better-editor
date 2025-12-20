use std::{
  borrow::Cow,
  path::{Path, PathBuf},
};

use kurbo::{Point, Rect, Vec2};

use crate::Render;

pub struct FileTree {
  tree:    Directory,
  focused: bool,
}

#[derive(PartialOrd, PartialEq, Eq, Ord)]
enum Item {
  Directory(Directory),
  File(File),
}

#[derive(Eq)]
struct Directory {
  path:     PathBuf,
  items:    Option<Vec<Item>>,
  expanded: bool,
}

#[derive(Eq)]
struct File {
  name: String,
}

impl PartialEq for File {
  fn eq(&self, other: &Self) -> bool { self.name == other.name }
}

impl PartialOrd for File {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Ord for File {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.name.cmp(&other.name) }
}

impl PartialEq for Directory {
  fn eq(&self, other: &Self) -> bool { self.name() == other.name() }
}

impl PartialOrd for Directory {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Ord for Directory {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.name().cmp(&other.name()) }
}

impl FileTree {
  pub fn current_directory() -> Self { FileTree::new(Path::new(".")) }

  pub fn on_focus(&mut self, focus: bool) { self.focused = focus; }

  pub fn new(path: &Path) -> Self {
    let path = path.canonicalize().unwrap();
    let mut tree = Directory::new(path);
    tree.expand();

    FileTree { tree, focused: false }
  }
}

impl Directory {
  fn new(path: PathBuf) -> Directory { Directory { path, items: None, expanded: false } }

  fn name(&self) -> Cow<'_, str> { self.path.file_name().unwrap().to_string_lossy() }

  fn expand(&mut self) {
    self.expanded = true;
    if self.items.is_none() {
      self.populate();
    }
  }

  fn populate(&mut self) {
    let mut items = vec![];

    for entry in std::fs::read_dir(&self.path).unwrap() {
      let entry = entry.unwrap();
      let path = entry.path();
      if path.is_dir() {
        items.push(Item::Directory(Directory::new(path)));
      } else {
        items
          .push(Item::File(File { name: path.file_name().unwrap().to_string_lossy().to_string() }));
      }
    }

    items.sort_unstable();

    self.items = Some(items);
  }
}

impl FileTree {
  pub fn draw(&self, render: &mut Render) {
    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background_lower,
    );

    self.tree.draw(Point::ZERO, render);
  }
}

impl Directory {
  fn draw(&self, pos: Point, render: &mut Render) -> f64 {
    render.fill(
      &Rect::new(pos.x, pos.y, render.size().width, pos.y + 20.0),
      render.theme().background_raised,
    );

    let text = render.layout_text(
      &format!(" {}", self.name()),
      pos + Vec2::new(20.0, 0.0),
      render.theme().text,
    );
    render.draw_text(&text);

    if self.expanded
      && let Some(items) = &self.items
    {
      let mut y = 20.0;
      for item in items {
        match item {
          Item::File(file) => {
            file.draw(pos + Vec2::new(20.0, y), render);
            y += 20.0;
          }
          Item::Directory(dir) => {
            y += dir.draw(pos + Vec2::new(20.0, y), render);
          }
        }
      }
      y
    } else {
      20.0
    }
  }
}
impl File {
  fn draw(&self, pos: Point, render: &mut Render) {
    let text = render.layout_text(&self.name, pos + Vec2::new(20.0, 0.0), render.theme().text);
    render.draw_text(&text);
  }
}
