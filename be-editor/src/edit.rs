use std::ops::Range;

use be_doc::Change;
use be_input::{Direction, Mode, Move, VerticalDirection};
use unicode_segmentation::UnicodeSegmentation;

use crate::{CommandMode, EditorState};

impl EditorState {
  pub(crate) fn perform_edit(&mut self, e: be_input::Edit) {
    use be_input::Edit;

    if let Some(command) = &mut self.command {
      if matches!(e, Edit::Insert('\n')) {
        self.run_command();
        self.set_mode(Mode::Normal);
        return;
      }

      command.perform_edit(e);
      if command.mode == CommandMode::Search {
        self.search_text = Some(command.text.clone());
        self.damage_all = true;
      }

      return;
    }

    match e {
      Edit::Insert(c) => {
        let mut bytes = [0; 4];
        let s = c.encode_utf8(&mut bytes);
        self.change(Change::insert(self.doc.cursor_offset(self.cursor), s));
        self.move_graphemes(1);

        match c {
          '\n' => {
            self.trim_line(self.cursor.line - 1);
            self.auto_indent(VerticalDirection::Up)
          }
          '}' | ']' | ')' => self.fix_indent(),
          _ => {}
        }
      }
      Edit::Replace(c) => {
        let mut bytes = [0; 4];
        let s = c.encode_utf8(&mut bytes);
        self.change(Change::replace(self.doc.grapheme_slice(self.cursor, 1), s));
      }
      Edit::Delete(m) => self.perform_delete_move(m),
      Edit::Cut(m) => {
        self.set_mode(Mode::Insert);
        self.perform_delete_move(m);
      }
      Edit::DeleteLine => {
        let end = if self.cursor.line == self.max_line() {
          if self.cursor.line == 0 {
            self.doc.byte_of_line_end(self.cursor.line)
          } else {
            self.doc.rope.byte_len()
          }
        } else {
          self.doc.byte_of_line(self.cursor.line + 1)
        };

        self.delete_copy(self.doc.byte_of_line(self.cursor.line)..end);
        self.clamp_cursor();
      }
      Edit::CutLine => {
        self.set_mode(Mode::Insert);
        self.delete_copy(
          self.doc.byte_of_line(self.cursor.line)..self.doc.byte_of_line_end(self.cursor.line),
        );
        self.clamp_column();

        self.auto_indent(VerticalDirection::Up);
      }
      Edit::DeleteRestOfLine => {
        self.delete_copy(
          self.doc.cursor_offset(self.cursor)
            ..self.doc.offset_by_graphemes(self.doc.byte_of_line(self.cursor.line + 1), -1),
        );
        self.clamp_column();
      }
      Edit::Paste { after } => self.paste(after),
      Edit::Backspace => {
        if self.doc.cursor_offset(self.cursor) > 0 {
          self.move_graphemes(-1);
          self.change(Change::remove(self.doc.grapheme_slice(self.cursor, 1)));
        }
      }
      Edit::Undo => {
        if self.history_position < self.history.len() {
          self.history_position += 1;
          for change in self.history[self.history.len() - self.history_position].clone().undo() {
            self.keep_cursor_for_change(change);
            self.change_no_history(change.clone());
          }
          self.clamp_cursor();
        }
      }
      Edit::Redo => {
        if self.history_position > 0 {
          for change in self.history[self.history.len() - self.history_position].clone().redo() {
            self.keep_cursor_for_change(change);
            self.change_no_history(change.clone());
          }
          self.history_position -= 1;
          self.clamp_cursor();
        }
      }
      Edit::SwitchCase => {
        let range = self.doc.grapheme_slice(self.cursor, 1);
        if let Some(c @ ('a'..='z' | 'A'..='Z')) = self.doc.range(range.clone()).chars().next() {
          let c = ((c as u8) ^ 0x20) as char;
          let mut buf = [0; 4];
          let s = c.encode_utf8(&mut buf);
          self.change(Change::replace(range, s));
        }

        self.move_col_rel(1);
        self.clamp_cursor();
      }
    }
  }

  // Perform the move after 'd' or 'c'.
  fn perform_delete_move(&mut self, m: Move) {
    if matches!(m, Move::Single(Direction::Right)) {
      let range = self.doc.grapheme_slice(self.cursor, 1);
      if !self.doc.range(range.clone()).chars().any(|c| c == '\n') {
        self.delete_copy(range);
      }
      return;
    }

    let inclusive = match m {
      Move::EndWord => true,
      _ => false,
    };

    let start = self.doc.cursor_offset(self.cursor);
    self.perform_move(m);
    if inclusive {
      self.move_graphemes(1);
    }
    let end = self.doc.cursor_offset(self.cursor);

    let change = Change::remove(start..end);
    self.keep_cursor_for_change(&change);
    self.delete_copy(start..end);
  }

  /// Copy the given range, then delete it, then fix the cursor. This is used
  /// for all the 'd*' and 'c*' commands.
  fn delete_copy(&mut self, range: Range<usize>) {
    self.copy(range.clone());
    self.change(Change::remove(range));
    self.clamp_cursor();
  }

  fn copy(&mut self, range: Range<usize>) {
    let text = self.doc.range(range);
    self.copied = text.to_string();
  }

  fn paste(&mut self, after: bool) {
    if self.copied.chars().any(|c| c == '\n') {
      // Multiline copies are inserted at the end of the line.
      let idx = if after {
        self.doc.byte_of_line((self.cursor.line + 1).clamp(self.max_line()))
      } else {
        self.doc.byte_of_line(self.cursor.line)
      };

      self.change(Change::insert(idx, &self.copied));
      if after {
        self.move_line_rel(self.copied.chars().filter(|c| *c == '\n').count() as i32);
      }
    } else {
      if after {
        self.move_graphemes(1);
      }

      self.change(Change::insert(self.doc.cursor_offset(self.cursor), &self.copied));
      self.move_graphemes(self.copied.graphemes(true).count().saturating_sub(1) as isize);
    }
  }
}

#[cfg(test)]
mod tests {
  use be_input::{Direction, Edit, Move};

  use crate::tests::editor;

  #[test]
  fn delete_wont_remove_newline() {
    let mut editor = editor("foo\nbar\n");

    editor.perform_move(Move::LineEnd);

    editor.check_repeated(
      |e| e.perform_edit(Edit::Delete(Move::Single(Direction::Right))),
      &[
        expect![@r#"
          fo⟦o⟧
          bar
        "#],
        expect![@r#"
          f⟦o⟧
          bar
        "#],
        expect![@r#"
          ⟦f⟧
          bar
        "#],
        expect![@r#"
          ⟦ ⟧
          bar
        "#],
        expect![@r#"
          ⟦ ⟧
          bar
        "#],
      ],
    );

    editor.perform_move(Move::Single(Direction::Down));

    editor.check_repeated(
      |e| e.perform_edit(Edit::Delete(Move::Single(Direction::Right))),
      &[
        expect![@r#"

          ⟦b⟧ar
        "#],
        expect![@r#"

          ⟦a⟧r
        "#],
        expect![@r#"

          ⟦r⟧
        "#],
        expect![@r#"

          ⟦ ⟧
        "#],
        expect![@r#"

          ⟦ ⟧
        "#],
      ],
    );
  }

  #[test]
  fn backspace_stops_at_start() {
    let mut editor = editor("foo\nbar\n");
    editor.perform_move(Move::LineEnd);
    editor.perform_action(be_input::Action::SetMode { mode: be_input::Mode::Insert, delta: 1 });
    editor.check_repeated(
      |e| e.perform_edit(Edit::Backspace),
      &[
        expect![@r#"
          foo‖
          bar
        "#],
        expect![@r#"
          fo‖
          bar
        "#],
        expect![@r#"
          f‖
          bar
        "#],
        expect![@r#"
          ‖
          bar
        "#],
        expect![@r#"
          ‖
          bar
        "#],
      ],
    )
  }

  #[test]
  fn delete_line_at_end() {
    let mut editor = editor("foo\nbar\n");
    editor.perform_move(Move::FileEnd);
    editor.check_repeated(
      |e| e.perform_edit(Edit::DeleteLine),
      &[
        expect![@r#"
          foo
          ⟦b⟧ar
        "#],
        expect![@r#"
          ⟦f⟧oo
        "#],
        expect![@r#"
          ⟦ ⟧
        "#],
      ],
    );
  }
}
