use be_input::{Action, Mode};
use kurbo::Rect;

use crate::{Render, Updater};

mod editor;
mod file_tree;
mod search;
mod shell;

pub use editor::EditorView;
pub use file_tree::FileTree;
pub use search::Search;
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
  Search(Search),
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
      ViewContent::Shell(_) => false,
      ViewContent::Search(_) => false,
    }
  }

  pub fn update(&mut self, updater: &mut Updater) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.editor.update(),
      ViewContent::FileTree(_) => {}
      ViewContent::Shell(shell) => shell.update(updater),
      ViewContent::Search(search) => search.update(),
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    if !self.visible() {
      return;
    }

    match &mut self.content {
      ViewContent::Editor(editor) => editor.draw(render),
      ViewContent::FileTree(file_tree) => file_tree.draw(render),
      ViewContent::Shell(shell) => shell.draw(render),
      ViewContent::Search(search) => search.draw(render),
    }
  }

  pub fn mode(&self) -> be_input::Mode {
    match &self.content {
      ViewContent::Editor(editor) => editor.editor.mode(),
      ViewContent::FileTree(_) => Mode::Normal,
      ViewContent::Shell(_) => Mode::Insert,
      ViewContent::Search(_) => Mode::Insert,
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match &mut self.content {
      ViewContent::Editor(editor) => editor.editor.perform_action(action),
      ViewContent::FileTree(file_tree) => file_tree.perform_action(action),
      ViewContent::Shell(shell) => shell.perform_action(action),
      ViewContent::Search(search) => search.perform_action(action),
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
      ViewContent::Shell(shell) => shell.on_focus(focus),
      ViewContent::Search(_) => {}
    }
  }
}
