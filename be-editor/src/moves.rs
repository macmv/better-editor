use be_doc::{Column, Line};
use be_input::{ChangeDirection, Direction, Move};

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

      Move::MatchingBracket => {
        let offset = self.doc.cursor_offset(self.cursor);

        #[derive(Default)]
        struct BraceCollector {
          // Signed because braces can be mismatched.
          parens:   i32,
          brackets: i32,
          braces:   i32,
        }

        impl BraceCollector {
          fn visit(&mut self, c: char) {
            match c {
              '(' => self.parens += 1,
              ')' => self.parens -= 1,
              '[' => self.brackets += 1,
              ']' => self.brackets -= 1,
              '{' => self.braces += 1,
              '}' => self.braces -= 1,
              _ => {}
            }
          }

          fn count_of(&self, c: char) -> i32 {
            match c {
              '(' | ')' => self.parens,
              '[' | ']' => self.brackets,
              '{' | '}' => self.braces,
              _ => unreachable!(),
            }
          }
        }

        let mut search_forward = true;
        let mut found = None;

        let mut i = offset;
        for c in self.doc.range(offset..).chars() {
          match c {
            '\n' => {
              break;
            }
            '(' | '[' | '{' => {
              search_forward = true;
              found = Some((i, opposite_brace(c)));
              break;
            }
            ')' | ']' | '}' => {
              search_forward = false;
              found = Some((i + c.len_utf8(), opposite_brace(c)));
              break;
            }

            _ => i += c.len_utf8(),
          }
        }

        if let Some((found, to_match)) = found {
          let slice =
            if search_forward { self.doc.range(found..) } else { self.doc.range(..found) };
          let iter = MaybeReversed::from_forward(slice.chars(), search_forward);

          let mut collector = BraceCollector::default();

          let mut matched = false;
          let mut index = found;
          for c in iter {
            collector.visit(c);
            if c == to_match && collector.count_of(to_match) == 0 {
              matched = true;
              if !search_forward {
                index -= c.len_utf8();
              }
              break;
            }

            if search_forward {
              index += c.len_utf8();
            } else {
              index -= c.len_utf8();
            }
          }

          if matched {
            self.cursor = self.doc.offset_to_cursor(index);
            self.clamp_cursor();
          }
        }
      }

      Move::Result(dir) => {
        if let Some(search) = self.search_text.as_ref() {
          let offset = self.doc.cursor_offset(self.cursor) + 1;
          if let Some(res) = match dir {
            ChangeDirection::Next => self.doc.find_from(offset, search).next(),
            ChangeDirection::Prev => self.doc.rfind_from(offset, search).next(),
          } {
            let cursor = self.doc.offset_to_cursor(res);
            self.cursor = cursor;
          }
        }
      }

      Move::Change(dir) => {
        if let Some(changes) = &self.changes {
          if let Some(line) = match dir {
            ChangeDirection::Next => changes.next_hunk(self.cursor.line),
            ChangeDirection::Prev => changes.prev_hunk(self.cursor.line),
          } {
            self.move_to_line(line);
          }
        }
      }

      Move::Diagnostic(dir) => {
        let offset = self.doc.cursor_offset(self.cursor);

        if let Some(d) = match dir {
          ChangeDirection::Next => self.lsp.diagnostics.iter().find(|d| d.range.start > offset),
          ChangeDirection::Prev => self.lsp.diagnostics.iter().rfind(|d| d.range.start < offset),
        } {
          let cursor = self.doc.offset_to_cursor(d.range.start);
          self.move_to_line(cursor.line);
          self.move_to_col(cursor.column);
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

enum MaybeReversed<T> {
  Forward(T),
  Reversed(std::iter::Rev<T>),
}

impl<T> MaybeReversed<T>
where
  T: DoubleEndedIterator,
{
  fn from_forward(iter: T, forward: bool) -> MaybeReversed<T> {
    if forward { MaybeReversed::Forward(iter) } else { MaybeReversed::Reversed(iter.rev()) }
  }
}

impl<T> Iterator for MaybeReversed<T>
where
  T: DoubleEndedIterator,
{
  type Item = T::Item;

  fn next(&mut self) -> Option<Self::Item> {
    match self {
      MaybeReversed::Forward(iter) => iter.next(),
      MaybeReversed::Reversed(iter) => iter.next(),
    }
  }
}

fn opposite_brace(c: char) -> char {
  match c {
    '(' => ')',
    ')' => '(',
    '[' => ']',
    ']' => '[',
    '{' => '}',
    '}' => '{',
    _ => unreachable!(),
  }
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

  #[test]
  fn matching_bracket() {
    let mut editor = editor("fn foo(bar)");
    editor.check_repeated(
      |e| e.perform_move(Move::MatchingBracket),
      &[expect![@"fn foo(bar⟦)⟧"], expect![@"fn foo⟦(⟧bar)"], expect![@"fn foo(bar⟦)⟧"]],
    );
  }

  #[test]
  fn matching_bracket_nested() {
    let mut editor = editor("fn foo(bar(baz))");
    editor.check_repeated(
      |e| e.perform_move(Move::MatchingBracket),
      &[
        expect![@"fn foo(bar(baz)⟦)⟧"],
        expect![@"fn foo⟦(⟧bar(baz))"],
        expect![@"fn foo(bar(baz)⟦)⟧"],
      ],
    );

    editor.perform_move(Move::Single(be_input::Direction::Left));
    editor.check(expect![@"fn foo(bar(baz⟦)⟧)"]);
    editor.check_repeated(
      |e| e.perform_move(Move::MatchingBracket),
      &[
        expect![@"fn foo(bar⟦(⟧baz))"],
        expect![@"fn foo(bar(baz⟦)⟧)"],
        expect![@"fn foo(bar⟦(⟧baz))"],
      ],
    );
  }

  #[test]
  fn matching_bracket_multi_line() {
    let mut editor = editor("fn foo{\n  bar\n}\n");
    editor.check_repeated(
      |e| e.perform_move(Move::MatchingBracket),
      &[
        expect![@r#"
          fn foo{
            bar
          ⟦}⟧
        "#],
        expect![@r#"
          fn foo⟦{⟧
            bar
          }
        "#],
        expect![@r#"
          fn foo{
            bar
          ⟦}⟧
        "#],
      ],
    );

    editor.perform_move(Move::Single(be_input::Direction::Up));
    editor.check(expect![@r#"
      fn foo{
      ⟦ ⟧ bar
      }
    "#]);
    editor.perform_move(Move::MatchingBracket);
    editor.check(expect![@r#"
      fn foo{
      ⟦ ⟧ bar
      }
    "#]);
  }
}
