use be_editor::EditorState;
use be_input::{Action, Key, KeyStroke, Mode};
use kurbo::{Axis, Point, Rect};

use crate::{CursorMode, Distance, Render, TextLayout, file_tree::FileTree};

pub struct Editor {
  root: Pane,
}

enum Pane {
  Content(Content),
  Split(Split),
}

enum Content {
  Editor(EditorView),
  FileTree(FileTree),
}

struct EditorView {
  editor: EditorState,

  scroll:  Point,
  focused: bool,
}

struct Split {
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
  fn draw(&mut self, render: &mut Render) {
    match self {
      Pane::Content(content) => content.draw(render),
      Pane::Split(split) => split.draw(render),
    }
  }

  fn active(&self) -> &Content {
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
    }
  }

  fn mode(&self) -> Mode {
    match self {
      Content::Editor(editor) => editor.editor.mode(),
      Content::FileTree(_) => Mode::Normal,
    }
  }

  fn perform_action(&mut self, action: Action) {
    match self {
      Content::Editor(editor) => editor.editor.perform_action(action),
      Content::FileTree(file_tree) => file_tree.perform_action(action),
    }
  }

  fn on_focus(&mut self, focus: bool) {
    match self {
      Content::Editor(editor) => editor.on_focus(focus),
      Content::FileTree(file_tree) => file_tree.on_focus(focus),
    }
  }
}

impl Editor {
  pub fn new() -> Self {
    Editor {
      root: Pane::Split(Split {
        axis:    Axis::Vertical,
        percent: 0.2,
        active:  Side::Right,
        left:    Box::new(Pane::Content(Content::FileTree(FileTree::current_directory()))),
        right:   Box::new(Pane::Content(Content::Editor(EditorView {
          editor:  EditorState::from("💖hello\n💖foobar\nsdjkhfl\nworld\n"),
          scroll:  Point::ZERO,
          focused: true,
        }))),
      }),
    }
  }

  pub fn open(&mut self, path: &std::path::Path) {
    match self.root.active_mut() {
      Content::Editor(editor) => {
        let _ = editor.editor.open(path);
      }
      Content::FileTree(_) => {}
    }
  }

  pub fn on_key(&mut self, keys: &[KeyStroke]) -> Result<(), be_input::ActionError> {
    if keys.get(0).is_some_and(|k| k.control && k.key == 'w') {
      if keys.len() == 1 {
        return Err(be_input::ActionError::Incomplete);
      }

      match keys[1].key {
        Key::Char('h') => {
          self.root.focus(Direction::Left);
        }
        Key::Char('j') => {
          self.root.focus(Direction::Down);
        }
        Key::Char('k') => {
          self.root.focus(Direction::Up);
        }
        Key::Char('l') => {
          self.root.focus(Direction::Right);
        }
        _ => {}
      }

      return Ok(());
    }

    let action = Action::from_input(self.root.active().mode(), keys)?;
    self.root.active_mut().perform_action(action);

    Ok(())
  }

  pub fn draw(&mut self, render: &mut Render) { self.root.draw(render); }
}

#[derive(Copy, Clone)]
enum Direction {
  Up,
  Down,
  Left,
  Right,
}

impl EditorView {
  fn on_focus(&mut self, focus: bool) { self.focused = focus; }

  pub fn draw(&mut self, render: &mut Render) {
    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background,
    );

    let line_height = render.store.text.font_metrics().line_height;

    let min_line = ((self.scroll.y / line_height).floor() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());
    let max_line = (((self.scroll.y + render.size().height) / line_height).ceil() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());

    let start = self.editor.doc().rope.byte_of_line(min_line);
    let end = if max_line >= self.editor.doc().rope.line_len() {
      self.editor.doc().rope.byte_len()
    } else {
      self.editor.doc().rope.byte_of_line(max_line + 1)
    };

    let mut index = start;
    let mut i = min_line;

    let mut y = 0.0;
    loop {
      let Some(mut layout) = self.layout_line(render, index) else { break };
      layout.set_pos(Point::new(20.0, y));

      if self.focused && self.editor.cursor().line == i {
        let mode = match self.editor.mode() {
          Mode::Normal | Mode::Visual => Some(CursorMode::Block),
          Mode::Insert => Some(CursorMode::Line),
          Mode::Replace => Some(CursorMode::Underline),
          Mode::Command => None,
        };

        if let Some(mode) = mode {
          let cursor = layout.cursor(self.editor.cursor_column_byte(), mode);
          render.fill(&cursor, render.theme().text);
        }
      }

      render.draw_text(&layout);

      y += line_height;
      i += 1;
      index += self.editor.doc().rope.byte_slice(index..).raw_lines().next().unwrap().byte_len();
      if index >= end {
        break;
      }
    }

    if let Some(command) = self.editor.command() {
      render.fill(
        &Rect::new(
          0.0,
          render.size().height - line_height,
          render.size().width,
          render.size().height,
        ),
        render.theme().background_raised,
      );

      let layout = render.layout_text(
        &command.text,
        (20.0, render.size().height - line_height),
        render.theme().text,
      );

      render.draw_text(&layout);

      let cursor = layout.cursor(command.cursor as usize, CursorMode::Line);
      render.fill(&cursor, render.theme().text);
    } else if let Some(status) = self.editor.status() {
      render.fill(
        &Rect::new(
          0.0,
          render.size().height - line_height,
          render.size().width,
          render.size().height,
        ),
        render.theme().background_raised,
      );

      let layout = render.layout_text(
        &status.message,
        (20.0, render.size().height - line_height),
        render.theme().text,
      );
      render.draw_text(&layout);
    }

    if let Some(ft) = self.editor.file_type() {
      let layout = render.layout_text(
        &format!("{ft}"),
        (render.size().width - 50.0, render.size().height - line_height),
        render.theme().text,
      );
      render.draw_text(&layout);
    }
  }

  fn layout_line(&mut self, render: &mut Render, index: usize) -> Option<TextLayout> {
    let line = self.editor.doc().rope.byte_slice(index..).raw_lines().next()?;
    let max_index = index + line.byte_len();

    let line_string = line.to_string();
    let theme = &render.store.theme;
    let mut layout =
      render.store.text.layout_builder(&line_string, render.theme().text, render.scale());

    let highlights = self.editor.highlights(index..max_index);
    let mut prev = index;
    for highlight in highlights {
      let pos = if highlight.pos > max_index { max_index } else { highlight.pos };

      if let Some(color) = theme.syntax.lookup(&highlight.highlights) {
        layout.color_range(prev - index..pos - index, color);
      }

      if highlight.pos > max_index {
        break;
      }

      prev = highlight.pos;
    }

    let layout = layout.build(&line_string);
    let layout = render.build_layout(layout, Point::new(20.0, 0.0));

    Some(layout)
  }
}
