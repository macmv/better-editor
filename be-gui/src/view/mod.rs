use be_input::{Action, Mode};

use crate::Render;

mod editor;
mod file_tree;
mod shell;

pub use editor::EditorView;
pub use file_tree::FileTree;
pub use shell::Shell;

pub enum View {
  Editor(EditorView),
  FileTree(FileTree),
  Shell(Shell),
}

impl View {
  pub fn draw(&mut self, render: &mut Render) {
    match self {
      View::Editor(editor) => editor.draw(render),
      View::FileTree(file_tree) => file_tree.draw(render),
      View::Shell(shell) => shell.draw(render),
    }
  }

  pub fn mode(&self) -> be_input::Mode {
    match self {
      View::Editor(editor) => editor.editor.mode(),
      View::FileTree(_) => Mode::Normal,
      View::Shell(_) => Mode::Insert,
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match self {
      View::Editor(editor) => editor.editor.perform_action(action),
      View::FileTree(file_tree) => file_tree.perform_action(action),
      View::Shell(shell) => shell.perform_action(action),
    }
  }

  pub fn on_focus(&mut self, focus: bool) {
    match self {
      View::Editor(editor) => editor.on_focus(focus),
      View::FileTree(file_tree) => file_tree.on_focus(focus),
      View::Shell(_) => {}
    }
  }
}
