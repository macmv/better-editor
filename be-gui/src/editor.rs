use be_editor::EditorState;
use be_input::{Action, Key};
use kurbo::{Point, Rect};

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

        render.fill(
          &Rect::new(
            20.0 + (self.editor.cursor().column.as_usize() as f64) * CHAR_WIDTH,
            y,
            20.0 + (self.editor.cursor().column.as_usize() as f64 + 1.0) * CHAR_WIDTH,
            y + self.line_height,
          ),
          oklch(1.0, 0.0, 0.0),
        );
      }

      y += self.line_height;
    }
  }
}
