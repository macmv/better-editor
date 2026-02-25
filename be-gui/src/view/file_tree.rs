use std::{
  borrow::Cow,
  ops::{BitOr, BitOrAssign},
  path::{Path, PathBuf},
  sync::LazyLock,
};

use be_git::Repo;
use be_input::{Action, ChangeDirection, Direction, Mode, Move};
use be_shared::SharedHandle;
use kurbo::{Point, Rect, Vec2};

use crate::{
  Color, Layout, Notify, Render,
  icon::{self, Icon},
  theme::Theme,
};

pub struct FileTree {
  tree:    Directory,
  focused: bool,
  active:  usize,

  notify: Notify,
  repo:   SharedHandle<Option<be_git::Repo>>,
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

enum ItemMut<'a> {
  Directory(&'a mut Directory),
  File(&'a mut File),
}

#[derive(Eq)]
struct Directory {
  path:     PathBuf,
  items:    Option<Vec<Item>>,
  expanded: bool,

  status: Option<FileStatus>,
}

#[derive(Eq)]
struct File {
  name: String,
  path: PathBuf,

  status: Option<FileStatus>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum FileStatus {
  #[default]
  Unchanged,

  Created,
  Modified,
  Deleted,

  Ignored,
}

// TODO: Fix git calls to be:
//
// 1. On repo/workspace init:
//
// - store HEAD
//
// 2. On status refresh trigger (file save, fs notify debounce, manual refresh,
//    branch switch):
//
// - repo.statuses(Some(&mut opts)) with:
//   - include_untracked(true)
//   - recurse_untracked_dirs(true)
//   - include_unmodified(false)
// - Build file_status: HashMap<PathBuf, Status>.
// - Build dir_status: HashMap<PathBuf, AggregatedStatus> by folding each file
//   status up its parent dirs.
// - Render uses only these maps.
// - Maybe build a sum tree?
//
// 3. On file open (need HEAD state/content):
//
// - Check cache head_entry_cache[path].
// - If missing for current head_tree_oid:
//   - tree.get_path(rel) to know if it exists in HEAD (added/new vs tracked).
//   - If you need actual HEAD text, then find_blob(entry.id()) once and cache.
//
// 4. On HEAD change:
//
// - Detect by comparing new HEAD to cached.
// - Invalidate only HEAD-related caches (head_entry_cache, opened-file original
//   docs).
// - Recompute statuses once.

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
  pub fn current_directory(notify: Notify, workspace: &be_workspace::Workspace) -> Self {
    FileTree::new(Path::new("."), notify, workspace)
  }

  pub fn new(path: &Path, notify: Notify, workspace: &be_workspace::Workspace) -> Self {
    let path = path.canonicalize().unwrap();
    let mut tree = Directory::new(path);
    tree.expand();

    FileTree { tree, focused: false, active: 0, notify, repo: workspace.repo.clone() }
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
              curr.populate();
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
        Move::FileStart => self.active = 0,
        Move::FileEnd => self.active = self.tree.len_visible().saturating_sub(1),
        Move::Change(dir) => {
          self.move_until(dir, |s| {
            s.active_mut().map_or(false, |it| {
              matches!(
                it.status(),
                FileStatus::Created | FileStatus::Modified | FileStatus::Deleted
              )
            })
          });
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

  fn move_until(&mut self, dir: ChangeDirection, cond: impl Fn(&mut Self) -> bool) {
    loop {
      match dir {
        ChangeDirection::Next => {
          if self.active == self.tree.len_visible().saturating_sub(1) {
            break;
          }
          self.active += 1;
        }
        ChangeDirection::Prev => {
          if self.active == 0 {
            break;
          }
          self.active -= 1;
        }
      }

      if cond(self) {
        break;
      }
    }
  }
}

impl Directory {
  fn new(path: PathBuf) -> Self { Directory { path, items: None, expanded: false, status: None } }

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

  fn expand(&mut self) { self.expanded = true; }

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
          status: None,
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
  pub fn draw(&mut self, render: &mut Render) {
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
      active:       self.active,
      focused:      self.focused,
    }
    .draw_item(ItemRef::Directory(&self.tree), render);
  }

  pub fn layout(&mut self, _layout: &mut Layout) {
    puffin::profile_function!();

    let mut node = ItemMut::Directory(&mut self.tree);
    if let Some(repo) = &*self.repo {
      node.layout(&repo);
    }
  }

  pub fn on_focus(&mut self, focus: bool) { self.focused = focus; }
}

struct TreeDraw {
  line:   usize,
  indent: usize,

  indent_width: f64,
  line_height:  f64,

  active:  usize,
  focused: bool,
}

impl Item {
  fn as_ref(&self) -> ItemRef<'_> {
    match self {
      Item::File(f) => ItemRef::File(f),
      Item::Directory(d) => ItemRef::Directory(d),
    }
  }

  fn as_mut(&mut self) -> ItemMut<'_> {
    match self {
      Item::File(f) => ItemMut::File(f),
      Item::Directory(d) => ItemMut::Directory(d),
    }
  }
}

impl ItemMut<'_> {
  fn layout(&mut self, repo: &Repo) {
    match self {
      ItemMut::Directory(dir) => {
        if dir.expanded && dir.items.is_none() {
          dir.populate();
        }

        // TODO: Move the caching to repo? It's somewhat nice to have it here. We just
        // need some sense of 'staleness'.
        if dir.status.is_none() {
          if repo.is_ignored(&dir.path) {
            dir.status = Some(FileStatus::Ignored);
          } else if repo.is_added(&dir.path) {
            dir.status = Some(FileStatus::Created);
          } else if repo.is_modified(&dir.path) {
            dir.status = Some(FileStatus::Modified);
          } else {
            dir.status = Some(FileStatus::Unchanged);
          }
        }

        if let Some(items) = &mut dir.items {
          for it in items {
            it.as_mut().layout(repo);
            if let Some(stat) = &mut dir.status {
              *stat |= it.status();
            }
          }
        }
      }
      ItemMut::File(file) => file.layout(repo),
    }
  }
}

impl Item {
  fn status(&self) -> FileStatus {
    match self {
      Item::Directory(d) => d.status.unwrap_or(FileStatus::default()),
      Item::File(f) => f.status.unwrap_or(FileStatus::default()),
    }
  }
}

impl File {
  fn layout(&mut self, repo: &Repo) {
    if self.status.is_none() {
      if repo.is_ignored(&self.path) {
        self.status = Some(FileStatus::Ignored);
      } else if repo.is_added(&self.path) {
        self.status = Some(FileStatus::Created);
      } else if repo.is_modified(&self.path) {
        self.status = Some(FileStatus::Modified);
      } else {
        self.status = Some(FileStatus::Unchanged);
      }
    }
  }
}

impl TreeDraw {
  fn pos(&self) -> Point {
    Point::new(self.indent as f64 * self.indent_width, self.line as f64 * self.line_height)
  }

  fn draw_item(&mut self, item: ItemRef, render: &mut Render) {
    if self.active == self.line {
      if self.focused {
        render.fill(
          &Rect::new(0.0, self.pos().y, render.size().width, self.pos().y + self.line_height),
          render.theme().background_raised,
        );
      } else {
        render.stroke(
          &Rect::new(0.0, self.pos().y, render.size().width, self.pos().y + self.line_height),
          render.theme().background_raised,
          kurbo::Stroke::new(1.0),
        );
      }
    }

    match item {
      ItemRef::File(file) => self.draw_file(file, render),
      ItemRef::Directory(dir) => self.draw_directory(dir, render),
    }
  }

  fn draw_directory(&mut self, dir: &Directory, render: &mut Render) {
    let text = render.layout_text(&format!("{}", dir.name()), render.theme().text);

    let icon = if dir.expanded { &*icon::CHEVRON_DOWN } else { &*icon::CHEVRON_RIGHT };
    icon.stroke(
      self.pos() + Vec2::new(self.indent_width - 16.0, text.size().height / 2.0 - 6.0),
      12.0,
      render.theme().background_raised_outline,
      render,
    );

    icon::FOLDER.fill(
      self.pos() + Vec2::new(self.indent_width, text.size().height / 2.0 - 6.0),
      12.0,
      crate::oklch(0.7, 0.14, 240.0),
      render,
    );

    render.draw_text(&text, self.pos() + Vec2::new(self.indent_width + 16.0, 0.0));

    if let Some(status) = dir.status
      && let Some(icon) = status.icon()
    {
      icon.stroke(
        self.pos()
          + Vec2::new(
            self.indent_width + 16.0 + text.size().width + 4.0,
            text.size().height / 2.0 - 6.0,
          ),
        12.0,
        status.color(render.theme()),
        render,
      );
    }

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

    let (icon, fill, color) = match render
      .store
      .workspace
      .config
      .borrow()
      .language_for_filename(&file.name)
      .map(|lang| (lang, render.store.workspace.config.borrow()))
      .as_ref()
      .and_then(|(lang, config)| config.languages[&lang].icon.as_deref())
    {
      Some("rust") => (icon::RUST, true, crate::oklch(0.6534, 0.216925, 37.3651)),
      Some("markdown") => (icon::MARKDOWN, true, crate::oklch(1.0, 0.0, 0.0)),

      // TODO: Filetypes based on entire filename.
      _ if file.name == ".gitignore" => (icon::GIT, true, crate::oklch(0.6516, 0.2066, 34.17)),

      _ => (icon::TEXT_ALIGN_START, false, crate::oklch(1.0, 0.0, 0.0)),
    };

    if fill {
      icon.fill(
        self.pos() + Vec2::new(self.indent_width, text.size().height / 2.0 - 6.0),
        12.0,
        color,
        render,
      );
    } else {
      icon.stroke(
        self.pos() + Vec2::new(self.indent_width, text.size().height / 2.0 - 6.0),
        12.0,
        color,
        render,
      );
    }

    render.draw_text(&text, self.pos() + Vec2::new(self.indent_width + 16.0, 0.0));

    if let Some(status) = file.status
      && let Some(icon) = status.icon()
    {
      icon.stroke(
        self.pos()
          + Vec2::new(
            self.indent_width + 16.0 + text.size().width + 4.0,
            text.size().height / 2.0 - 6.0,
          ),
        12.0,
        status.color(render.theme()),
        render,
      );
    }
  }
}

impl FileStatus {
  fn icon(&self) -> Option<LazyLock<Icon>> {
    match self {
      FileStatus::Modified => Some(icon::SQUARE_DOT),
      FileStatus::Created => Some(icon::SQUARE_PLUS),
      FileStatus::Deleted => Some(icon::MINUS),

      FileStatus::Ignored => Some(icon::SQUARE_SLASH),

      _ => None,
    }
  }

  fn color(&self, theme: &Theme) -> Color {
    match self {
      FileStatus::Created => theme.diff_add,
      FileStatus::Modified => theme.diff_change,
      FileStatus::Deleted => theme.diff_remove,

      FileStatus::Ignored => theme.background_raised_outline,

      _ => theme.text,
    }
  }
}

impl BitOr for FileStatus {
  type Output = Self;

  fn bitor(self, rhs: Self) -> Self::Output {
    match (self, rhs) {
      (l, r) if l == r => l,
      (_, FileStatus::Modified) => FileStatus::Modified,
      (FileStatus::Modified, _) => FileStatus::Modified,

      (FileStatus::Unchanged, FileStatus::Ignored) => FileStatus::Unchanged,
      (FileStatus::Ignored, FileStatus::Unchanged) => FileStatus::Unchanged,

      (_, FileStatus::Ignored) => FileStatus::Ignored,
      (FileStatus::Ignored, _) => FileStatus::Ignored,

      _ => FileStatus::Modified,
    }
  }
}

impl BitOrAssign for FileStatus {
  fn bitor_assign(&mut self, rhs: Self) { *self = *self | rhs; }
}
