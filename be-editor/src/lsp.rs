use std::{cell::RefCell, rc::Rc};

use be_lsp::{LanguageClientState, LanguageServerKey, command, types};
use be_task::Task;

use crate::EditorState;

#[derive(Default)]
pub struct LspState {
  pub store:  Rc<RefCell<be_lsp::LanguageServerStore>>,
  pub client: LanguageClientState,

  document_version: i32,
  pub completions:  CompletionsState,

  // FIXME: ew.
  pub set_waker: bool,
}

#[derive(Default)]
pub struct CompletionsState {
  tasks:            Vec<Task<Option<types::CompletionResponse>>>,
  completions:      types::CompletionList,
  show:             bool,
  clear_on_message: bool,
}

impl EditorState {
  pub(crate) fn connect_to_lsp(&mut self) {
    let Some(ft) = &self.filetype else { return };
    let config = self.config.borrow();
    let Some(language) = config.language.get(ft.name()) else { return };
    let Some(lsp) = &language.lsp else { return };

    let key = LanguageServerKey::Language(ft.name().into());

    let server = {
      let mut store = self.lsp.store.borrow_mut();
      match store.get(&key) {
        Some(server) => server,
        None => store.spawn(key.clone(), &lsp.command),
      }
    };
    self.lsp.client.set(key, server);

    self.lsp.client.send(&command::DidOpenTextDocument {
      path:        self.file.as_ref().unwrap().path().to_path_buf(),
      text:        self.doc.rope.to_string(),
      language_id: "rust".into(),
    });
  }

  pub(crate) fn lsp_notify_change(&mut self, change: crate::Change) {
    let range = types::Range {
      start: self.offset_to_lsp(change.range.start),
      end:   self.offset_to_lsp(change.range.end),
    };

    self.lsp.document_version += 1;

    self.lsp.client.send(&command::DidChangeTextDocument {
      path:    self.file.as_ref().unwrap().path().to_path_buf(),
      version: self.lsp.document_version,
      changes: vec![(range, change.text)],
    });
  }

  pub(crate) fn lsp_request_completions(&mut self) {
    let cursor = self.cursor_to_lsp();

    let tasks = self.lsp.client.send(&command::Completion {
      path: self.file.as_ref().unwrap().path().to_path_buf(),
      cursor,
    });
    self.lsp.completions.clear_on_message = true;
    self.lsp.completions.tasks = tasks;
  }

  pub fn completions(&mut self) -> Option<Vec<String>> {
    self.lsp.completions.tasks.retain(|task| {
      if let Some(completed) = task.completed() {
        if self.lsp.completions.clear_on_message {
          self.lsp.completions.completions.items.clear();
          self.lsp.completions.clear_on_message = false;
        }

        if let Some(completions) = completed {
          self.lsp.completions.completions.items.extend(match completions {
            types::CompletionResponse::List(list) => list.items,
            types::CompletionResponse::Array(completions) => completions,
          });
        }
        self.lsp.completions.show = true;
        false
      } else {
        true
      }
    });

    if self.lsp.completions.show {
      Some(self.lsp.completions.completions.items.iter().map(|i| i.label.clone()).collect())
    } else {
      None
    }
  }

  fn cursor_to_lsp(&self) -> types::Position {
    types::Position {
      line:      self.cursor.line.as_usize() as u32,
      character: self.doc.cursor_column_offset(self.cursor) as u32,
    }
  }

  fn offset_to_lsp(&self, offset: usize) -> types::Position {
    let line = self.doc.rope.line_of_byte(offset);
    let column = offset - self.doc.rope.byte_of_line(line);
    types::Position { line: line as u32, character: column as u32 }
  }
}
