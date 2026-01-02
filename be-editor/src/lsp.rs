use std::{cell::RefCell, ops::Range, rc::Rc};

use be_doc::{Change, Edit};
use be_lsp::{LanguageClientState, LanguageServerKey, command, types};
use be_task::Task;

use crate::{EditorState, HighlightKey, highlight::Highlight};

#[derive(Default)]
pub struct LspState {
  pub store:  Rc<RefCell<be_lsp::LanguageServerStore>>,
  pub client: LanguageClientState,

  document_version:       i32,
  pub completions:        CompletionsState,
  pub(crate) diagnostics: Vec<Diagnostic>,

  // FIXME: ew.
  pub set_waker: bool,

  pub save_task: Option<SaveTask>,
}

#[derive(Default)]
pub struct CompletionsState {
  tasks:            Vec<Task<Option<types::CompletionResponse>>>,
  completions:      types::CompletionList,
  show:             bool,
  clear_on_message: bool,
}

pub struct SaveTask {
  task:    Task<Option<Vec<types::TextEdit>>>,
  started: std::time::Instant,
}

pub struct Diagnostic {
  pub range:   Range<usize>,
  pub message: String,
  pub level:   DiagnosticLevel,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticLevel {
  Error,
  Warning,
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

  pub(crate) fn lsp_update_diagnostics(&mut self) {
    let Some(file) = &self.file else { return };

    self.lsp.diagnostics.clear();
    self.lsp.client.servers(|state| {
      if let Some(d) = state.diagnostics.get(file.path()) {
        self.lsp.diagnostics.extend(d.iter().map(|d| Diagnostic {
          range:   lsp_to_offset(&self.doc, d.range.start)..lsp_to_offset(&self.doc, d.range.end),
          message: d.message.clone(),
          level:   match d.severity {
            Some(types::DiagnosticSeverity::ERROR) => DiagnosticLevel::Error,
            Some(types::DiagnosticSeverity::WARNING) => DiagnosticLevel::Warning,
            _ => DiagnosticLevel::Error,
          },
        }));
      }
    });

    self.lsp.diagnostics.sort_by_key(|d| d.range.start);
  }

  pub(crate) fn lsp_notify_change(&mut self, change: &crate::Change) {
    let Some(file) = &self.file else { return };

    let range = types::Range {
      start: self.offset_to_lsp(change.range.start),
      end:   self.offset_to_lsp(change.range.end),
    };

    self.lsp.document_version += 1;

    self.lsp.client.send(&command::DidChangeTextDocument {
      path:    file.path().to_path_buf(),
      version: self.lsp.document_version,
      changes: vec![(range, change.text.clone())],
    });
  }

  pub(crate) fn lsp_on_save(&mut self) {
    let Some(file) = &self.file else { return };

    let task = self
      .lsp
      .client
      .send_first_capable(&command::DocumentFormat { path: file.path().to_path_buf() });
    self.lsp.save_task = task.map(|t| SaveTask { task: t, started: std::time::Instant::now() });
  }

  pub(crate) fn lsp_finish_on_save(&mut self) {
    if let Some(task) = &self.lsp.save_task {
      if let Some(completed) = task.task.completed() {
        if let Some(edits) = completed {
          self.apply_bulk_lsp_edits(edits);
        }
        self.lsp.save_task = None;
      } else if task.started.elapsed() > std::time::Duration::from_millis(500) {
        // TODO: User-visible warning.
        log::warn!("LSP format on save timed out");
        self.lsp.save_task = None;
      }
    }
  }

  fn apply_bulk_lsp_edits(&mut self, edits: Vec<types::TextEdit>) {
    let single_edit = self.current_edit.is_none();
    if single_edit {
      self.current_edit = Some(Edit::empty());
    }

    // See https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textEditArray
    //
    // What I gather from this very overly-explained paragraph is, just iterate
    // in reverse.
    for edit in edits.into_iter().rev() {
      let start = lsp_to_offset(&self.doc, edit.range.start);
      let end = lsp_to_offset(&self.doc, edit.range.end);
      let change = Change { range: start..end, text: edit.new_text };
      self.keep_cursor_for_change(&change);
      self.change(change);
    }

    if single_edit {
      self.remove_current_edit();
    }
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
    let column = offset - self.doc.byte_of_line(be_doc::Line(line));
    types::Position { line: line as u32, character: column as u32 }
  }
}

fn lsp_to_offset(doc: &be_doc::Document, position: types::Position) -> usize {
  doc.byte_of_line(be_doc::Line(position.line as usize)) + position.character as usize
}

impl Diagnostic {
  pub fn highlight(&self) -> Highlight<'_> {
    Highlight {
      start: self.range.start,
      end:   self.range.end,
      key:   HighlightKey::Diagnostic(self.level),
    }
  }
}
