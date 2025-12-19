use be_editor::EditorState;
use be_input::{Action, Key, Mode};
use kurbo::{Point, Rect};

use crate::{CursorMode, Render, oklch};

pub struct Editor {
  editor: EditorState,

  line_height: f64,
  scroll:      Point,
}

impl Editor {
  pub fn new() -> Self {
    Editor {
      editor:      EditorState::from("hello\nworld\n"),
      line_height: 20.0,
      scroll:      Point::ZERO,
    }
  }

  pub fn on_key(&mut self, keys: &[Key]) -> Result<(), be_input::ActionError> {
    let action = Action::from_input(self.editor.mode(), keys)?;
    self.editor.perform_action(action);

    Ok(())
  }

  pub fn draw(&self, render: &mut Render) {
    render
      .fill(&Rect::new(0.0, 0.0, render.size().width, render.size().height), oklch(0.3, 0.0, 0.0));

    let min_line = ((self.scroll.y / self.line_height).floor() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());
    let max_line = (((self.scroll.y + render.size().height) / self.line_height).ceil() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());

    let mut y = 0.0;
    for (i, line) in
      self.editor.doc().rope.line_slice(min_line as usize..max_line as usize).lines().enumerate()
    {
      let layout = render.layout_text(&line.to_string(), (20.0, y), oklch(1.0, 0.0, 0.0));
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
          render.fill(&cursor, oklch(1.0, 0.0, 0.0));
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
        oklch(0.4, 0.0, 0.0),
      );

      let layout = render.layout_text(
        &command.text,
        (20.0, render.size().height - 40.0),
        oklch(1.0, 0.0, 0.0),
      );

      render.draw_text(&layout);

      let cursor = layout.cursor(command.cursor as usize, CursorMode::Line);
      render.fill(&cursor, oklch(1.0, 0.0, 0.0));
    }
  }
}
