use be_doc::{Column, Line};
use be_input::{Direction, Move};

use crate::EditorState;

impl EditorState {
  pub(crate) fn perform_move(&mut self, m: be_input::Move) {
    if let Some(command) = &mut self.command {
      command.perform_move(m);
      return;
    }

    match m {
      Move::Single(Direction::Left) => self.move_col_rel(-1),
      Move::Single(Direction::Right) => self.move_col_rel(1),
      Move::Single(Direction::Up) => self.move_line_rel(-1),
      Move::Single(Direction::Down) => self.move_line_rel(1),

      Move::LineEnd => self.move_to_col(Column::MAX),
      Move::LineStart => self.move_to_col(Column(0)),

      Move::FileStart => self.move_to_line(Line(0)),
      Move::FileEnd => self.move_to_line(self.max_line()),

      Move::NextWord => {
        if self.cursor_kind() != WordKind::Blank {
          let start = self.cursor_kind();
          while self.cursor_kind() == start {
            self.move_graphemes(1);
          }
        }

        while self.cursor_kind() == WordKind::Blank {
          self.move_graphemes(1);
        }
      }

      _ => {}
    }
  }

  fn cursor_char(&self) -> char {
    let line = self.doc.line(self.cursor.line);
    let Some(grapheme) = line.graphemes().skip(self.cursor.column.0).next() else { return '\0' };
    grapheme.chars().next().unwrap_or('\0')
  }

  fn cursor_kind(&self) -> WordKind { word_kind(self.cursor_char()) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordKind {
  Word,
  Punctuation,
  Blank,
}

fn word_kind(c: char) -> WordKind {
  match c {
    'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => WordKind::Word,
    ' ' | '\r' | '\n' | '\t' => WordKind::Blank,
    _ => WordKind::Punctuation,
  }
}
