use std::{
  borrow::Cow,
  path::{Path, PathBuf},
};

use be_input::{Action, Direction, Mode, Move};
use kurbo::{Point, Rect, Vec2};

use crate::{Notify, Render, icon};

pub struct FileTree {
  tree:    Directory,
  focused: bool,
  active:  usize,

  notify: Notify,
}

#[derive(PartialOrd, PartialEq, Eq, Ord)]
enum Item {
  Directory(Directory),
  File(File),
}

enum ItemRef<'a> {
  Directory(&'a Directory),
  File(&'a File),
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
  path: PathBuf,
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
  pub fn current_directory(notify: Notify) -> Self { FileTree::new(Path::new("."), notify) }

  pub fn on_focus(&mut self, focus: bool) { self.focused = focus; }

  pub fn new(path: &Path, notify: Notify) -> Self {
    let path = path.canonicalize().unwrap();
    let mut tree = Directory::new(path);
    tree.expand();

    FileTree { tree, focused: false, active: 0, notify }
  }

  fn active_mut(&mut self) -> Option<&mut Item> {
    fn visit_dir<'a>(
      dir: &'a mut Directory,
      index: &mut usize,
      active: usize,
    ) -> Option<&'a mut Item> {
      if dir.expanded {
        for item in dir.items.as_mut().unwrap() {
          *index += 1;
          if let Some(it) = visit_item(item, index, active) {
            return Some(it);
          }
        }
      }

      None
    }
    fn visit_item<'a>(
      item: &'a mut Item,
      index: &mut usize,
      active: usize,
    ) -> Option<&'a mut Item> {
      if *index == active {
        return Some(item);
      }

      match item {
        Item::Directory(dir) => visit_dir(dir, index, active),
        Item::File(_) => None,
      }
    }

    visit_dir(&mut self.tree, &mut 0, self.active)
  }

  pub fn open(&mut self, path: &Path) {
    let mut curr = &mut self.tree;
    let mut new_active = 0;

    let Ok(path) = path.strip_prefix(".") else { return };
    let mut components = path.components().peekable();

    while let Some(component) = components.next() {
      match component {
        std::path::Component::Normal(name) => {
          let Some(items) = curr.items.as_mut() else { return };
          let Some(i) = items.iter().position(|i| *i.name() == *name) else { return };
          new_active += i + 1;

          match &mut items[i] {
            Item::Directory(dir) => {
              curr = dir;
              curr.expand();
            }
            Item::File(_) => {
              // If we're done with the path, then break and update `active`. Otherwise, we
              // found a file early, and the path is invalid.
              if components.peek().is_none() {
                break;
              } else {
                return;
              }
            }
          }
        }

        _ => return,
      }
    }

    self.active = new_active;
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
          Some(Item::File(file)) => {
            let path = file.path.clone();
            self.notify.open_file(path);
          }
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
        items.push(Item::File(File {
          name: path.file_name().unwrap().to_string_lossy().to_string(),
          path,
        }));
      }
    }

    items.sort_unstable();

    self.items = Some(items);
  }
}

impl Item {
  fn name(&self) -> Cow<'_, str> {
    match self {
      Item::Directory(d) => d.name(),
      Item::File(f) => Cow::Borrowed(&f.name),
    }
  }

  fn visible_len(&self) -> usize {
    match self {
      Item::Directory(d) => d.len_visible(),
      Item::File(_) => 1,
    }
  }
}

impl FileTree {
  pub fn draw(&self, render: &mut Render) {
    puffin::profile_function!();

    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background_lower,
    );

    TreeDraw {
      line:         0,
      indent:       0,
      indent_width: 12.0,
      line_height:  render.store.text.font_metrics().line_height,
      active:       if self.focused { Some(self.active) } else { None },
    }
    .draw_item(ItemRef::Directory(&self.tree), render);
  }
}

struct TreeDraw {
  line:   usize,
  indent: usize,

  indent_width: f64,
  line_height:  f64,

  active: Option<usize>,
}

impl Item {
  fn as_ref(&self) -> ItemRef<'_> {
    match self {
      Item::File(f) => ItemRef::File(f),
      Item::Directory(d) => ItemRef::Directory(d),
    }
  }
}

impl TreeDraw {
  fn pos(&self) -> Point {
    Point::new(self.indent as f64 * self.indent_width, self.line as f64 * self.line_height)
  }

  fn draw_item(&mut self, item: ItemRef, render: &mut Render) {
    if self.active == Some(self.line) {
      render.fill(
        &Rect::new(0.0, self.pos().y, render.size().width, self.pos().y + self.line_height),
        render.theme().background_raised,
      );
    }

    match item {
      ItemRef::File(file) => self.draw_file(file, render),
      ItemRef::Directory(dir) => self.draw_directory(dir, render),
    }
  }

  fn draw_directory(&mut self, dir: &Directory, render: &mut Render) {
    let text = render.layout_text(&format!(" {}", dir.name()), render.theme().text);

    let icon = if dir.expanded { &*icon::CHEVRON_DOWN } else { &*icon::CHEVRON_RIGHT };
    icon.draw(
      self.pos() + Vec2::new(self.indent_width - 16.0, text.size().height / 2.0 - 4.0),
      8.0,
      render.theme().background_raised_outline,
      render,
    );

    render.draw_text(&text, self.pos() + Vec2::new(self.indent_width, 0.0));

    if dir.expanded
      && let Some(items) = &dir.items
    {
      for item in items {
        self.line += 1;
        self.indent += 1;
        self.draw_item(item.as_ref(), render);
        self.indent -= 1;
      }
    }
  }

  fn draw_file(&self, file: &File, render: &mut Render) {
    let text = render.layout_text(&file.name, render.theme().text);
    render.draw_text(&text, self.pos() + Vec2::new(self.indent_width, 0.0));
  }
}
