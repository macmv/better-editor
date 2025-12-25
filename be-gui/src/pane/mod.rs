use be_input::{Action, Direction, Mode, Navigation};
use kurbo::Axis;

use crate::{
  Distance, Render,
  pane::{editor::EditorView, file_tree::FileTree, shell::Shell},
};

mod editor;
mod file_tree;
mod shell;

pub enum Pane {
  Content(Content),
  Split(Split),
}

pub enum Content {
  Editor(EditorView),
  FileTree(FileTree),
  Shell(Shell),
}

pub struct Split {
  axis:    Axis,
  percent: f64,
  active:  Side,
  left:    Box<Pane>,
  right:   Box<Pane>,
}

#[derive(Copy, Clone)]
enum Side {
  Left,
  Right,
}

impl Pane {
  pub fn new_editor() -> Self {
    Pane::Split(Split {
      axis:    Axis::Vertical,
      percent: 0.2,
      active:  Side::Right,
      left:    Box::new(Pane::Content(Content::FileTree(FileTree::current_directory()))),
      right:   Box::new(Pane::Content(Content::Editor(EditorView::new()))),
    })
  }

  pub fn new_shell() -> Self { Pane::Content(Content::Shell(Shell::new())) }

  pub fn open(&mut self, path: &std::path::Path) {
    match self.active_mut() {
      Content::Editor(editor) => {
        let _ = editor.editor.open(path);
      }
      Content::FileTree(_) => {}
      Content::Shell(_) => {}
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    match self {
      Pane::Content(content) => content.draw(render),
      Pane::Split(split) => split.draw(render),
    }
  }

  pub fn active(&self) -> &Content {
    match self {
      Pane::Content(content) => content,
      Pane::Split(split) => split.active(),
    }
  }

  fn active_mut(&mut self) -> &mut Content {
    match self {
      Pane::Content(content) => content,
      Pane::Split(split) => split.active_mut(),
    }
  }

  fn focus(&mut self, direction: Direction) -> bool {
    match self {
      Pane::Content(_) => false,
      Pane::Split(split) => split.focus(direction),
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Navigate { nav: Navigation::Direction(dir) } => {
        self.focus(dir);
      }
      _ => self.active_mut().perform_action(action),
    }
  }
}

impl Split {
  fn draw(&mut self, render: &mut Render) {
    render.split(
      self,
      self.axis,
      Distance::Percent(self.percent),
      |state, render| state.left.draw(render),
      |state, render| state.right.draw(render),
    );
  }

  fn active(&self) -> &Content {
    match self.active {
      Side::Left => self.left.active(),
      Side::Right => self.right.active(),
    }
  }

  fn active_mut(&mut self) -> &mut Content {
    match self.active {
      Side::Left => self.left.active_mut(),
      Side::Right => self.right.active_mut(),
    }
  }

  /// Returns true if the focus changed.
  fn focus(&mut self, direction: Direction) -> bool {
    let focused = match self.active {
      Side::Left => &mut self.left,
      Side::Right => &mut self.right,
    };

    if !focused.focus(direction) {
      match (self.active, self.axis, direction) {
        (Side::Left, Axis::Vertical, Direction::Right) => self.active = Side::Right,
        (Side::Right, Axis::Vertical, Direction::Left) => self.active = Side::Left,
        (Side::Left, Axis::Horizontal, Direction::Down) => self.active = Side::Right,
        (Side::Right, Axis::Horizontal, Direction::Up) => self.active = Side::Left,

        _ => return false,
      }

      match self.active {
        Side::Left => {
          self.left.active_mut().on_focus(true);
          self.right.active_mut().on_focus(false);
        }
        Side::Right => {
          self.right.active_mut().on_focus(true);
          self.left.active_mut().on_focus(false);
        }
      }

      true
    } else {
      false
    }
  }
}

impl Content {
  fn draw(&mut self, render: &mut Render) {
    match self {
      Content::Editor(editor) => editor.draw(render),
      Content::FileTree(file_tree) => file_tree.draw(render),
      Content::Shell(shell) => shell.draw(render),
    }
  }

  pub fn mode(&self) -> be_input::Mode {
    match self {
      Content::Editor(editor) => editor.editor.mode(),
      Content::FileTree(_) => Mode::Normal,
      Content::Shell(_) => Mode::Insert,
    }
  }

  fn perform_action(&mut self, action: Action) {
    match self {
      Content::Editor(editor) => editor.editor.perform_action(action),
      Content::FileTree(file_tree) => file_tree.perform_action(action),
      Content::Shell(shell) => shell.perform_action(action),
    }
  }

  fn on_focus(&mut self, focus: bool) {
    match self {
      Content::Editor(editor) => editor.on_focus(focus),
      Content::FileTree(file_tree) => file_tree.on_focus(focus),
      Content::Shell(_) => {}
    }
  }
}
