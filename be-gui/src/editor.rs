use be_doc::{Cursor, Document};
use be_input::Mode;
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

  pub fn draw(&self, render: &mut Render) {
    render
      .fill(&Rect::new(0.0, 0.0, render.size().width, render.size().height), oklch(0.3, 0.0, 0.0));

    let min_line =
      ((self.scroll.y / self.line_height).floor() as usize).clamp(0, self.doc.rope.lines().len());
    let max_line = (((self.scroll.y + render.size().height) / self.line_height).ceil() as usize)
      .clamp(0, self.doc.rope.lines().len());

    let mut y = 0.0;
    for line in self.doc.rope.line_slice(min_line as usize..max_line as usize).lines() {
      let layout = render.layout_text(&line.to_string(), (20.0, y), oklch(1.0, 0.0, 0.0));
      render.draw_text(layout);
      y += self.line_height;
    }
  }
}
