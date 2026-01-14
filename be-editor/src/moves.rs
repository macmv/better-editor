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
          while self.cursor_kind() == start && !self.at_eof() {
            self.move_graphemes(1);
          }
        }

        while self.cursor_kind() == WordKind::Blank && !self.at_eof() {
          self.move_graphemes(1);
        }
      }

      Move::EndWord => {
        self.move_graphemes(1);
        let mut move_backward = true;
        while self.cursor_kind() == WordKind::Blank {
          if self.at_eof() {
            move_backward = false;
            break;
          }
          self.move_graphemes(1);
        }

        let start = self.cursor_kind();
        while self.cursor_kind() == start {
          if self.at_eof() {
            move_backward = false;
            break;
          }
          self.move_graphemes(1);
        }
        if move_backward {
          self.move_graphemes(-1);
        }
      }

      Move::PrevWord => {
        self.move_graphemes(-1);
        let mut move_forward = true;
        while self.cursor_kind() == WordKind::Blank {
          if self.at_start() {
            move_forward = false;
            break;
          }
          self.move_graphemes(-1);
        }

        let start = self.cursor_kind();
        while self.cursor_kind() == start {
          if self.at_start() {
            move_forward = false;
            break;
          }
          self.move_graphemes(-1);
        }
        if move_forward {
          self.move_graphemes(1);
        }
      }

      Move::NextResult => {
        if let Some(search) = self.search_text.as_ref() {
          if let Some(res) =
            self.doc.find_from(self.doc.cursor_offset(self.cursor) + 1, search).next()
          {
            let cursor = self.doc.offset_to_cursor(res);
            self.cursor = cursor;
          }
        }
      }

      Move::PrevResult => {
        if let Some(search) = self.search_text.as_ref() {
          if let Some(res) =
            self.doc.rfind_from(self.doc.cursor_offset(self.cursor) + 1, search).next()
          {
            let cursor = self.doc.offset_to_cursor(res);
            self.cursor = cursor;
          }
        }
      }

      _ => {}
    }
  }

  fn at_start(&self) -> bool { self.cursor.line == 0 && self.cursor.column == 0 }

  fn at_eof(&self) -> bool {
    self.cursor.line > self.max_line()
      || (self.cursor.line == self.max_line() && self.cursor.column >= self.max_column())
  }

  fn cursor_char(&self) -> char {
    let line = self.doc.line(self.cursor.line);
    let Some(grapheme) = line.graphemes().skip(self.cursor.column.0).next() else {
      return '\n';
    };
    grapheme.chars().next().unwrap_or('\n')
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

#[cfg(test)]
mod tests {
  use crate::tests::*;
  use be_input::Move;

  #[test]
  fn next_word() {
    let mut editor = editor("fn foo() -> Self { bar }");
    editor.check(expect![@"⟦f⟧n foo() -> Self { bar }"]);

    editor.check_repeated(
      |e| e.perform_move(Move::NextWord),
      &[
        expect![@"fn ⟦f⟧oo() -> Self { bar }"],
        expect![@"fn foo⟦(⟧) -> Self { bar }"],
        expect![@"fn foo() ⟦-⟧> Self { bar }"],
        expect![@"fn foo() -> ⟦S⟧elf { bar }"],
        expect![@"fn foo() -> Self ⟦{⟧ bar }"],
        expect![@"fn foo() -> Self { ⟦b⟧ar }"],
        expect![@"fn foo() -> Self { bar ⟦}⟧"],
        expect![@"fn foo() -> Self { bar ⟦}⟧"],
      ],
    );
  }

  #[test]
  fn end_word() {
    let mut editor = editor("fn foo() -> Self { bar }");
    editor.check(expect![@"⟦f⟧n foo() -> Self { bar }"]);

    editor.check_repeated(
      |e| e.perform_move(Move::EndWord),
      &[
        expect![@"f⟦n⟧ foo() -> Self { bar }"],
        expect![@"fn fo⟦o⟧() -> Self { bar }"],
        expect![@"fn foo(⟦)⟧ -> Self { bar }"],
        expect![@"fn foo() -⟦>⟧ Self { bar }"],
        expect![@"fn foo() -> Sel⟦f⟧ { bar }"],
        expect![@"fn foo() -> Self ⟦{⟧ bar }"],
        expect![@"fn foo() -> Self { ba⟦r⟧ }"],
        expect![@"fn foo() -> Self { bar ⟦}⟧"],
        expect![@"fn foo() -> Self { bar ⟦}⟧"],
      ],
    );
  }

  #[test]
  fn prev_word() {
    let mut editor = editor("fn foo() -> Self { bar }");
    editor.perform_move(Move::LineEnd);
    editor.check(expect![@"fn foo() -> Self { bar ⟦}⟧"]);

    editor.check_repeated(
      |e| e.perform_move(Move::PrevWord),
      &[
        expect![@"fn foo() -> Self { ⟦b⟧ar }"],
        expect![@"fn foo() -> Self ⟦{⟧ bar }"],
        expect![@"fn foo() -> ⟦S⟧elf { bar }"],
        expect![@"fn foo() ⟦-⟧> Self { bar }"],
        expect![@"fn foo⟦(⟧) -> Self { bar }"],
        expect![@"fn ⟦f⟧oo() -> Self { bar }"],
        expect![@"⟦f⟧n foo() -> Self { bar }"],
        expect![@"⟦f⟧n foo() -> Self { bar }"],
      ],
    );
  }
}
