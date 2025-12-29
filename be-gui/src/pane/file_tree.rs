use std::{
  borrow::Cow,
  path::{Path, PathBuf},
};

use be_input::{Action, Direction, Mode, Move};
use kurbo::{Point, Rect, Vec2};

use crate::Render;

pub struct FileTree {
  tree:    Directory,
  focused: bool,
  active:  usize,
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

    FileTree { tree, focused: false, active: 0 }
  }

  fn active_mut(&mut self) -> Option<&mut Item> {
    fn visit_dir(dir: &mut Directory, mut index: usize, active: usize) -> Option<&mut Item> {
      if dir.expanded {
        for item in dir.items.as_mut().unwrap() {
          index += 1;
          if let Some(it) = visit_item(item, index, active) {
            return Some(it);
          }
        }
      }

      None
    }
    fn visit_item(item: &mut Item, index: usize, active: usize) -> Option<&mut Item> {
      if index == active {
        return Some(item);
      }

      match item {
        Item::Directory(dir) => visit_dir(dir, index, active),
        Item::File(_) => None,
      }
    }

    visit_dir(&mut self.tree, 0, self.active)
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { count: _, m } => match m {
        Move::Single(Direction::Up) => self.active = self.active.saturating_sub(1),
        Move::Single(Direction::Down) => {
          self.active = self.active.saturating_add(1).min(self.tree.len_visible().saturating_sub(1))
        }
        _ => (),
      },
      Action::Append { .. } | Action::SetMode { mode: Mode::Insert, .. } => {
        match self.active_mut() {
          Some(Item::Directory(dir)) => dir.toggle_expanded(),
          Some(Item::File(_)) => {}
          None => {}
        }
      }

      _ => {}
    }
  }
}

impl Directory {
  fn new(path: PathBuf) -> Directory { Directory { path, items: None, expanded: false } }

  fn name(&self) -> Cow<'_, str> { self.path.file_name().unwrap().to_string_lossy() }

  fn len_visible(&self) -> usize {
    if self.expanded {
      self.items.as_ref().map(|i| i.iter().map(|i| i.visible_len()).sum::<usize>()).unwrap_or(0) + 1
    } else {
      1
    }
  }

  fn toggle_expanded(&mut self) {
    if self.expanded {
      self.expanded = false;
    } else {
      self.expand();
    }
  }

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

impl Item {
  fn visible_len(&self) -> usize {
    match self {
      Item::Directory(d) => d.len_visible(),
      Item::File(_) => 1,
    }
  }
}

impl FileTree {
  pub fn draw(&self, render: &mut Render) {
    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background_lower,
    );

    TreeDraw { line: 0, indent: 0, active: if self.focused { Some(self.active) } else { None } }
      .draw_directory(&self.tree, render);
  }
}

struct TreeDraw {
  line:   usize,
  indent: usize,

  active: Option<usize>,
}

impl TreeDraw {
  fn pos(&self) -> Point { Point::new(self.indent as f64 * 20.0, self.line as f64 * 20.0) }

  fn draw_directory(&mut self, dir: &Directory, render: &mut Render) {
    if self.active == Some(self.line) {
      render.fill(
        &Rect::new(0.0, self.pos().y, render.size().width, self.pos().y + 20.0),
        render.theme().background_raised,
      );
    }

    let text = render.layout_text(&format!(" {}", dir.name()), render.theme().text);
    render.draw_text(&text, self.pos() + Vec2::new(20.0, 0.0));

    if dir.expanded
      && let Some(items) = &dir.items
    {
      for item in items {
        self.line += 1;
        self.indent += 1;
        match item {
          Item::File(file) => self.draw_file(file, render),
          Item::Directory(dir) => self.draw_directory(dir, render),
        }
        self.indent -= 1;
      }
    }
  }

  fn draw_file(&self, file: &File, render: &mut Render) {
    if self.active == Some(self.line) {
      render.fill(
        &Rect::new(0.0, self.pos().y, render.size().width, self.pos().y + 20.0),
        render.theme().background_raised,
      );
    }

    let text = render.layout_text(&file.name, render.theme().text);
    render.draw_text(&text, self.pos() + Vec2::new(20.0, 0.0));
  }
}
