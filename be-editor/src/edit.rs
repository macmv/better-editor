use be_doc::Change;
use be_input::{Mode, VerticalDirection};

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
      Edit::Delete | Edit::Cut => {
        let range = self.doc.grapheme_slice(self.cursor, 1);
        if !self.doc.range(range.clone()).chars().any(|c| c == '\n') {
          self.change(Change::remove(range));
          self.clamp_column();
        }

        if matches!(e, Edit::Cut) {
          self.set_mode(Mode::Insert);
        }
      }
      Edit::DeleteLine => {
        self.change(Change::remove(
          self.doc.byte_of_line(self.cursor.line)..self.doc.byte_of_line(self.cursor.line + 1),
        ));
        self.clamp_column();
      }
      Edit::CutLine => {
        self.set_mode(Mode::Insert);
        self.change(Change::remove(
          self.doc.byte_of_line(self.cursor.line)..self.doc.byte_of_line_end(self.cursor.line),
        ));
        self.clamp_column();

        self.auto_indent(VerticalDirection::Up);
      }
      Edit::DeleteRestOfLine => {
        self.change(Change::remove(
          self.doc.cursor_offset(self.cursor)
            ..self.doc.offset_by_graphemes(self.doc.byte_of_line(self.cursor.line + 1), -1),
        ));
        self.clamp_column();
      }
      Edit::Backspace => {
        self.move_graphemes(-1);
        self.change(Change::remove(self.doc.grapheme_slice(self.cursor, 1)));
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
    editor.check(expect![@r#"
      fo⟦o⟧
      bar
    "#
    ]);

    editor.check_repeated(
      |e| e.perform_edit(Edit::Delete),
      &[
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
    editor.check(expect![@r#"

      ⟦b⟧ar
    "#]);

    editor.check_repeated(
      |e| e.perform_edit(Edit::Delete),
      &[
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
}
