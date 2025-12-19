use be_editor::EditorState;
use be_input::{Action, Key, Mode};
use kurbo::{Axis, Point, Rect};

use crate::{CursorMode, Render, file_tree::FileTree, theme::Theme};

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

  line_height: f64,
  scroll:      Point,
}

struct Split {
  axis:   Axis,
  active: Side,
  left:   Box<Pane>,
  right:  Box<Pane>,
}

enum Side {
  Left,
  Right,
}

impl Pane {
  fn draw(&self, render: &mut Render) {
    match self {
      Pane::Content(content) => content.draw(render),
      Pane::Split(split) => split.draw(render),
    }
  }

  fn active(&self) -> &Content {
    match self {
      Pane::Content(content) => content,
      Pane::Split(split) => match split.active {
        Side::Left => split.left.active(),
        Side::Right => split.right.active(),
      },
    }
  }

  fn active_mut(&mut self) -> &mut Content {
    match self {
      Pane::Content(content) => content,
      Pane::Split(split) => match split.active {
        Side::Left => split.left.active_mut(),
        Side::Right => split.right.active_mut(),
      },
    }
  }
}

impl Split {
  fn draw(&self, render: &mut Render) {
    let (left, right) = match self.axis {
      Axis::Vertical => (
        Rect::new(0.0, 0.0, render.size().width / 2.0, render.size().height),
        Rect::new(render.size().width / 2.0, 0.0, render.size().width, render.size().height),
      ),
      Axis::Horizontal => (
        Rect::new(0.0, 0.0, render.size().width, render.size().height / 2.0),
        Rect::new(0.0, render.size().height / 2.0, render.size().width, render.size().height),
      ),
    };

    render.clip(left);
    self.left.draw(render);
    render.pop_clip();
    render.clip(right);
    self.right.draw(render);
    render.pop_clip();
  }
}

impl Content {
  fn draw(&self, render: &mut Render) {
    match self {
      Content::Editor(editor) => editor.draw(render),
      Content::FileTree(_) => {}
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
      Content::FileTree(_) => {}
    }
  }
}

impl Editor {
  pub fn new() -> Self {
    Editor {
      root: Pane::Split(Split {
        axis:   Axis::Vertical,
        active: Side::Right,
        left:   Box::new(Pane::Content(Content::FileTree(FileTree::new()))),
        right:  Box::new(Pane::Content(Content::Editor(EditorView {
          editor:      EditorState::from("hello\nworld\n"),
          line_height: 20.0,
          scroll:      Point::ZERO,
        }))),
      }),
    }
  }

  pub fn on_key(&mut self, keys: &[Key]) -> Result<(), be_input::ActionError> {
    let action = Action::from_input(self.root.active().mode(), keys)?;
    self.root.active_mut().perform_action(action);

    Ok(())
  }

  pub fn draw(&self, render: &mut Render) { self.root.draw(render); }
}

impl EditorView {
  pub fn draw(&self, render: &mut Render) {
    let theme = Theme::current();

    render.fill(&Rect::new(0.0, 0.0, render.size().width, render.size().height), theme.background);

    let min_line = ((self.scroll.y / self.line_height).floor() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());
    let max_line = (((self.scroll.y + render.size().height) / self.line_height).ceil() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());

    let mut y = 0.0;
    for (i, line) in
      self.editor.doc().rope.line_slice(min_line as usize..max_line as usize).lines().enumerate()
    {
      let layout = render.layout_text(&line.to_string(), (20.0, y), theme.text);
      render.draw_text(&layout);

      if self.editor.cursor().line == i + min_line {
        let mode = match self.editor.mode() {
          Mode::Normal | Mode::Visual => Some(CursorMode::Block),
          Mode::Insert => Some(CursorMode::Line),
          Mode::Replace => Some(CursorMode::Underline),
          Mode::Command => None,
        };

        if let Some(mode) = mode {
          let cursor = layout.cursor(self.editor.cursor_column_byte(), mode);
          render.fill(&cursor, theme.text);
        }
      }

      y += self.line_height;
    }

    if let Some(command) = self.editor.command() {
      render.fill(
        &Rect::new(
          0.0,
          render.size().height - 40.0,
          render.size().width,
          render.size().height - 20.0,
        ),
        theme.background_raised,
      );

      let layout =
        render.layout_text(&command.text, (20.0, render.size().height - 40.0), theme.text);

      render.draw_text(&layout);

      let cursor = layout.cursor(command.cursor as usize, CursorMode::Line);
      render.fill(&cursor, theme.text);
    }
  }
}
