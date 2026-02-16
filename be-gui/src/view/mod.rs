use be_input::{Action, Mode};
use kurbo::{Rect, Size};

use crate::{Layout, Render, Widget};

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
      ViewContent::FileTree(_) => false,
      ViewContent::Terminal(_) => false,
    }
  }

  pub fn layout(&mut self, layout: &mut Layout) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.editor.layout(),
      ViewContent::FileTree(_) => {}
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
      ViewContent::Editor(editor) => editor.editor.perform_action(action),
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
    ev: crate::MouseEvent,
    size: Size,
    scale: f64,
  ) -> Option<crate::CursorKind> {
    match &mut self.content {
      ViewContent::Editor(editor) => Some(editor.on_mouse(ev, size, scale)),
      ViewContent::FileTree(_) => None,
      ViewContent::Terminal(_) => None,
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
