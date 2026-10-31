use be_input::{Action, Mode};
use kurbo::{Rect, Size};

use crate::{Layout, Render, RenderStore};

mod command;
mod editor;
mod file_tree;
mod search;
mod terminal;

pub use command::CommandView;
pub use editor::EditorView;
pub use file_tree::FileTree;
pub use search::Search;
pub use terminal::TerminalView;

pub struct View {
  pub content: ViewContent,
  pub bounds:  Rect,
}

#[derive(Clone, Copy, Default)]
pub struct Scroll {
  pub offset:      f64,
  pub line_height: f64,
}

impl From<EditorView> for ViewContent {
  fn from(value: EditorView) -> Self { ViewContent::Editor(value) }
}
impl From<FileTree> for ViewContent {
  fn from(value: FileTree) -> Self { ViewContent::FileTree(value) }
}
impl From<TerminalView> for ViewContent {
  fn from(value: TerminalView) -> Self { ViewContent::Terminal(value) }
}

impl<T: Into<ViewContent>> From<T> for View {
  fn from(value: T) -> Self { View::new(value) }
}

pub enum ViewContent {
  Editor(EditorView),
  FileTree(FileTree),
  Terminal(TerminalView),
}

pub enum Popup {
  Search(Search),
  Command(CommandView),
}

impl View {
  pub fn new(content: impl Into<ViewContent>) -> Self {
    View { content: content.into(), bounds: Rect::ZERO }
  }

  pub fn visible(&self) -> bool { !self.bounds.is_zero_area() }

  pub fn animated(&self) -> bool {
    match &self.content {
      ViewContent::Editor(editor) => editor.animated(),
      ViewContent::FileTree(tree) => tree.animated(),
      ViewContent::Terminal(_) => false,
    }
  }

  pub fn layout(&mut self, layout: &mut Layout) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.layout(layout),
      ViewContent::FileTree(tree) => tree.layout(layout),
      ViewContent::Terminal(terminal) => terminal.layout(layout),
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    if !self.visible() {
      return;
    }

    match &mut self.content {
      ViewContent::Editor(editor) => editor.draw(render),
      ViewContent::FileTree(file_tree) => file_tree.draw(render),
      ViewContent::Terminal(terminal) => terminal.draw(render),
    }
  }

  pub fn mode(&self) -> be_input::Mode {
    match &self.content {
      ViewContent::Editor(editor) => editor.editor.mode(),
      ViewContent::FileTree(_) => Mode::Normal,
      ViewContent::Terminal(_) => Mode::Insert,
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.perform_action(action),
      ViewContent::FileTree(file_tree) => file_tree.perform_action(action),
      ViewContent::Terminal(terminal) => terminal.perform_action(action),
    }
  }

  pub fn on_visible(&mut self, visible: bool) {
    if !visible {
      self.bounds = Rect::ZERO;
    }
  }

  pub fn on_focus(&mut self, focus: bool) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.on_focus(focus),
      ViewContent::FileTree(file_tree) => file_tree.on_focus(focus),
      ViewContent::Terminal(terminal) => terminal.on_focus(focus),
    }
  }

  pub(crate) fn on_mouse(
    &mut self,
    ev: &crate::MouseEvent,
    size: Size,
    store: &RenderStore,
  ) -> Option<crate::CursorKind> {
    match &mut self.content {
      ViewContent::Editor(editor) => Some(editor.on_mouse(ev, size, store)),
      ViewContent::FileTree(_) => None,
      ViewContent::Terminal(terminal) => Some(terminal.on_mouse(ev, size, store)),
    }
  }
}

impl Popup {
  pub fn bounds(&self, size: Size) -> Rect {
    match self {
      Popup::Search(_) => Rect::new(100.0, 50.0, size.width - 100.0, size.height - 50.0),
      Popup::Command(_) => {
        Rect::new(100.0, size.height - 110.0, size.width - 100.0, size.height - 50.0)
      }
    }
  }

  pub fn layout(&mut self, _layout: &mut Layout) {
    match self {
      Popup::Search(search) => search.layout(),
      Popup::Command(_) => {}
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    match self {
      Popup::Search(search) => search.draw(render),
      Popup::Command(command) => command.draw(render),
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match self {
      Popup::Search(search) => search.perform_action(action),
      Popup::Command(command) => command.perform_action(action),
    }
  }
}

impl Scroll {
  pub fn new(line_height: f64) -> Self { Scroll { offset: 0.0, line_height } }

  /// A negative number between -line_height and zero. This is the Y offset to
  /// draw the first line at.
  pub fn first_line_offset(&self) -> f64 { -(self.offset % self.line_height) }

  /// This is the main interaction for keyboard-driven scrolling. When the
  /// current row changes, this should be called, and it will change the scroll
  /// offset to ensure `row` is entirely visible.
  pub fn make_visible(&mut self, visible_height: f64, row: usize) {
    let (min, max) = self.fully_visible_range(visible_height);

    if row < min {
      self.offset = row as f64 * self.line_height;
    } else if row > max {
      self.offset = (row + 1) as f64 * self.line_height - visible_height;
    }
  }

  pub fn fully_visible_range(&self, visible_height: f64) -> (usize, usize) {
    let min = (self.offset / self.line_height).ceil() as usize;
    let max = ((self.offset + visible_height) / self.line_height).floor() as usize - 1;

    (min, max)
  }

  pub fn partially_visible_range(&self, visible_height: f64) -> (usize, usize) {
    let min = (self.offset / self.line_height).floor() as usize;
    let max = ((self.offset + visible_height) / self.line_height).ceil() as usize;

    (min, max)
  }

  pub fn partially_visible_lines(&self, visible_height: f64) -> impl Iterator<Item = (usize, f64)> {
    let (min, max) = self.partially_visible_range(visible_height);
    let offset = self.first_line_offset();

    (min..=max).map(move |line| (line, offset + (line - min) as f64 * self.line_height))
  }
}
