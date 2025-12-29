use be_input::{Action, Mode};
use kurbo::Rect;

use crate::Render;

mod editor;
mod file_tree;
mod shell;

pub use editor::EditorView;
pub use file_tree::FileTree;
pub use shell::Shell;

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
impl From<Shell> for ViewContent {
  fn from(value: Shell) -> Self { ViewContent::Shell(value) }
}

impl<T: Into<ViewContent>> From<T> for View {
  fn from(value: T) -> Self { View::new(value) }
}

pub enum ViewContent {
  Editor(EditorView),
  FileTree(FileTree),
  Shell(Shell),
}

impl View {
  pub fn new(content: impl Into<ViewContent>) -> Self {
    View { content: content.into(), bounds: Rect::ZERO }
  }

  pub fn draw(&mut self, render: &mut Render) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.draw(render),
      ViewContent::FileTree(file_tree) => file_tree.draw(render),
      ViewContent::Shell(shell) => shell.draw(render),
    }
  }

  pub fn mode(&self) -> be_input::Mode {
    match &self.content {
      ViewContent::Editor(editor) => editor.editor.mode(),
      ViewContent::FileTree(_) => Mode::Normal,
      ViewContent::Shell(_) => Mode::Insert,
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.editor.perform_action(action),
      ViewContent::FileTree(file_tree) => file_tree.perform_action(action),
      ViewContent::Shell(shell) => shell.perform_action(action),
    }
  }

  pub fn on_focus(&mut self, focus: bool) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.on_focus(focus),
      ViewContent::FileTree(file_tree) => file_tree.on_focus(focus),
      ViewContent::Shell(_) => {}
    }
  }
}
