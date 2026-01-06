use be_doc::Change;
use be_input::{Mode, VerticalDirection};

use crate::EditorState;

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
      Edit::Delete => {
        let range = self.doc.grapheme_slice(self.cursor, 1);
        if !self.doc.range(range.clone()).chars().any(|c| c == '\n') {
          self.change(Change::remove(range));
          self.clamp_column();
        }
      }
      Edit::DeleteLine => {
        self.change(Change::remove(
          self.doc.byte_of_line(self.cursor.line)..self.doc.byte_of_line(self.cursor.line + 1),
        ));
        self.clamp_column();
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
