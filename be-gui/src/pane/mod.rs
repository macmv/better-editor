use be_input::{Action, Direction, Mode, Navigation};
use kurbo::{Axis, Point, Rect};

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
  percent: Vec<f64>,
  active:  usize,
  items:   Vec<Pane>,
}

impl Pane {
  pub fn new_editor() -> Self {
    Pane::Split(Split {
      axis:    Axis::Vertical,
      percent: vec![0.2],
      active:  1,
      items:   vec![
        Pane::Content(Content::FileTree(FileTree::current_directory())),
        Pane::Content(Content::Editor(EditorView::new())),
      ],
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
    let mut bounds = Rect::from_origin_size(Point::ZERO, render.size());

    match self.axis {
      Axis::Vertical => {
        for (i, item) in self.items.iter_mut().enumerate() {
          let percent =
            self.percent.get(i).copied().unwrap_or_else(|| 1.0 - self.percent.iter().sum::<f64>());
          let mut distance = Distance::Percent(percent).to_pixels_in(render.size().width);
          if distance < 0.0 {
            distance += render.size().width;
          }

          bounds.x1 = bounds.x0 + distance;
          render.clipped(bounds, |render| item.draw(render));
          bounds.x0 += distance;
        }
      }

      Axis::Horizontal => {
        for (i, item) in self.items.iter_mut().enumerate() {
          let percent =
            self.percent.get(i).copied().unwrap_or_else(|| 1.0 - self.percent.iter().sum::<f64>());
          let mut distance = Distance::Percent(percent).to_pixels_in(render.size().width);
          if distance < 0.0 {
            distance += render.size().width;
          }

          bounds.y1 = bounds.y0 + distance;
          render.clipped(bounds, |render| item.draw(render));
          bounds.y0 += distance;
        }
      }
    }
  }

  fn active(&self) -> &Content { self.items[self.active].active() }

  fn active_mut(&mut self) -> &mut Content { self.items[self.active].active_mut() }

  /// Returns true if the focus changed.
  fn focus(&mut self, direction: Direction) -> bool {
    let focused = &mut self.items[self.active];

    if !focused.focus(direction) {
      let prev_active = self.active;
      match (self.axis, direction) {
        (Axis::Vertical, Direction::Right) if self.active < self.items.len() - 1 => {
          self.active += 1
        }
        (Axis::Vertical, Direction::Left) if self.active > 0 => self.active -= 1,
        (Axis::Horizontal, Direction::Down) if self.active < self.items.len() - 1 => {
          self.active += 1
        }
        (Axis::Horizontal, Direction::Up) if self.active > 0 => self.active -= 1,

        _ => return false,
      }

      self.items[prev_active].active_mut().on_focus(false);
      self.items[self.active].active_mut().on_focus(true);

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
