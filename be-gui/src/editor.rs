use be_doc::{Cursor, Document};
use be_input::{Action, Key, Mode};
use kurbo::{Point, Rect};

use crate::{Render, oklch};

pub struct Editor {
  doc:    Document,
  cursor: Cursor,
  mode:   Mode,

  line_height: f64,
  scroll:      Point,
}

impl Editor {
  pub fn new() -> Self {
    Editor {
      doc:         Document::from("hello\nworld\n"),
      cursor:      Cursor::START,
      mode:        Mode::Normal,
      line_height: 20.0,
      scroll:      Point::ZERO,
    }
  }

  pub fn on_key(&mut self, keys: &[Key]) -> Result<(), be_input::ActionError> {
    let action = Action::from_input(self.mode, keys)?;
    self.perform_action(action);

    Ok(())
  }

  fn perform_action(&mut self, action: Action) {
    match action {
      Action::SetMode(m) => self.mode = m,
      Action::Move { count: _, m } => self.perform_move(m),
      Action::Edit { count: _, e } => self.perform_edit(e),
    }
  }

  fn perform_move(&mut self, m: be_input::Move) {
    match m {
      be_input::Move::Left => self.cursor = self.doc.move_col(self.cursor, -1),
      be_input::Move::Right => self.cursor = self.doc.move_col(self.cursor, 1),
      be_input::Move::Up => self.cursor = self.doc.move_row(self.cursor, -1),
      be_input::Move::Down => self.cursor = self.doc.move_row(self.cursor, 1),

      _ => {}
    }
  }
  fn perform_edit(&mut self, e: be_input::Edit) {}

  pub fn draw(&self, render: &mut Render) {
    render
      .fill(&Rect::new(0.0, 0.0, render.size().width, render.size().height), oklch(0.3, 0.0, 0.0));

    let min_line =
      ((self.scroll.y / self.line_height).floor() as usize).clamp(0, self.doc.rope.lines().len());
    let max_line = (((self.scroll.y + render.size().height) / self.line_height).ceil() as usize)
      .clamp(0, self.doc.rope.lines().len());

    let mut y = 0.0;
    for (i, line) in
      self.doc.rope.line_slice(min_line as usize..max_line as usize).lines().enumerate()
    {
      let layout = render.layout_text(&line.to_string(), (20.0, y), oklch(1.0, 0.0, 0.0));
      render.draw_text(layout);

      if self.cursor.line == i + min_line {
        const CHAR_WIDTH: f64 = 8.0;

        render.fill(
          &Rect::new(
            20.0 + (self.cursor.column.as_usize() as f64) * CHAR_WIDTH,
            y,
            20.0 + (self.cursor.column.as_usize() as f64 + 1.0) * CHAR_WIDTH,
            y + self.line_height,
          ),
          oklch(1.0, 0.0, 0.0),
        );
      }

      y += self.line_height;
    }
  }
}
