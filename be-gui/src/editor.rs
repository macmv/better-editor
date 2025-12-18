use be_editor::EditorState;
use be_input::{Action, Key, Mode};
use kurbo::{Point, Rect, Vec2};

use crate::{Render, oklch};

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
      render.draw_text(layout);

      if self.editor.cursor().line == i + min_line {
        const CHAR_WIDTH: f64 = 8.0;

        let mut cursor = match self.editor.mode() {
          Mode::Normal | Mode::Visual => Rect::new(0.0, 0.0, CHAR_WIDTH, self.line_height),
          Mode::Insert | Mode::Command => Rect::new(0.0, 0.0, 1.0, self.line_height),
          Mode::Replace => Rect::new(0.0, self.line_height - 1.0, CHAR_WIDTH, self.line_height),
        };

        cursor = cursor
          + Vec2::new(20.0 + (self.editor.cursor().column.as_usize() as f64) * CHAR_WIDTH, y);

        render.fill(&cursor, oklch(1.0, 0.0, 0.0));
      }

      y += self.line_height;
    }
  }
}
