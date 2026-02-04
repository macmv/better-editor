use std::{cell::RefCell, ops::Range, rc::Rc};

use be_doc::{Change, Edit, crop::RopeSlice};
use be_lsp::{LanguageClientState, LanguageServerKey, TextEdit, command, types};
use be_task::Task;

use crate::{EditorState, HighlightKey, highlight::Highlight};

#[derive(Default)]
pub struct LspState {
  pub store:  Rc<RefCell<be_lsp::LanguageServerStore>>,
  pub client: LanguageClientState,

  document_version:       u32,
  pub completions:        CompletionsState,
  pub goto_definition:
    Option<Task<Option<types::Or2<types::Definition, Vec<types::LocationLink>>>>>,
  pub(crate) diagnostics: Vec<Diagnostic>,

  // FIXME: ew.
  pub set_waker: bool,

  pub save_task: Option<SaveTask>,
}

#[derive(Default)]
pub struct CompletionsState {
  tasks: Vec<Task<Option<types::Or2<Vec<types::CompletionItem>, types::CompletionList>>>>,
  completions:      types::CompletionList,
  show:             bool,
  clear_on_message: bool,
}

pub struct SaveTask {
  task:    Task<Vec<TextEdit>>,
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
    let Some(ft) = self.filetype else { return };
    let config = self.config.borrow();
    let Some(language) = config.languages.get(&ft) else { return };
    let Some(lsp) = &language.lsp else { return };

    let key = LanguageServerKey::Language(ft);

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
      doc:         self.doc.clone(),
      language_id: "rust".into(),
    });
  }

  pub(crate) fn lsp_update_diagnostics(&mut self) {
    let Some(file) = &self.file else { return };

    self.lsp.diagnostics.clear();
    self.lsp.client.servers(|state| {
      if let Some(file) = state.files.get(file.path()) {
        self.lsp.diagnostics.extend(file.diagnostics.iter().map(|d| Diagnostic {
          range:   d.range.clone(),
          message: d.message.clone(),
          level:   match d.severity {
            Some(types::DiagnosticSeverity::Error) => DiagnosticLevel::Error,
            Some(types::DiagnosticSeverity::Warning) => DiagnosticLevel::Warning,
            _ => DiagnosticLevel::Error,
          },
        }));
      }
    });

    for range in self.lsp.diagnostics.iter().map(|d| d.range.clone()).collect::<Vec<_>>() {
      self.damage_range(range);
    }

    self.lsp.diagnostics.sort_by_key(|d| d.range.start);
  }

  pub(crate) fn lsp_update_goto_definition(&mut self) {
    let Some(file) = &self.file else { return };

    if let Some(task) = &self.lsp.goto_definition {
      if let Some(Some(res)) = task.completed() {
        match res {
          types::Or2::A(def) => match def {
            types::Definition::Many(defs) => {
              if defs.len() == 1 {
                let Some(def_path) = defs[0].uri.to_file_path() else { return };

                let pos = crate::lsp::command::decode_position(
                  be_lsp::command::PositionEncoding::Utf8,
                  &self.doc,
                  defs[0].range.start.clone(),
                );

                if file.path() == def_path {
                  let cursor = self.doc.offset_to_cursor(pos);
                  self.move_to_line(cursor.line);
                  self.move_to_col(cursor.column);
                } else {
                  if let Some(send) = &self.send {
                    send(crate::EditorEvent::OpenFile(def_path));
                  }
                }
              }
            }
            types::Definition::Location(loc) => {
              warn!("unhandled definintion: {loc:?}");
            }
          },
          types::Or2::B(locs) => {
            warn!("unhandled definition: {locs:?}");
          }
        }

        self.lsp.goto_definition = None;
      }
    }
  }

  pub(crate) fn lsp_notify_change(&mut self, change: &crate::Change) {
    let Some(file) = &self.file else { return };

    self.lsp.document_version += 1;

    self.lsp.client.send(&command::DidChangeTextDocument {
      path:              file.path().to_path_buf(),
      version:           self.lsp.document_version,
      doc_before_change: self.doc.clone(),
      changes:           vec![change.clone()],
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
      if let Some(edits) = task.task.completed() {
        self.apply_bulk_lsp_edits(edits);
        self.lsp.save_task = None;
      } else if task.started.elapsed() > std::time::Duration::from_millis(500) {
        // TODO: User-visible warning.
        log::warn!("LSP format on save timed out");
        self.lsp.save_task = None;
      }
    }
  }

  pub(crate) fn lsp_notify_did_save(&mut self) {
    let Some(file) = &self.file else { return };

    self.lsp.client.send(&command::DidSaveTextDocument { path: file.path().to_path_buf() });
  }

  fn apply_bulk_lsp_edits(&mut self, edits: Vec<TextEdit>) {
    if edits.is_empty() {
      return;
    }

    let single_edit = self.current_edit.is_none();
    if single_edit {
      self.current_edit = Some(Edit::empty());
    }

    // See https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textEditArray
    //
    // What I gather from this very overly-explained paragraph is, just iterate
    // in reverse.
    for edit in edits.into_iter().rev() {
      let change = Change { range: edit.range, text: edit.new_text };
      self.keep_cursor_for_change(&change);
      self.change(change);
    }

    if single_edit {
      self.remove_current_edit();
    }
  }

  pub(crate) fn lsp_request_completions(&mut self) {
    let tasks = self.lsp.client.send(&command::Completion {
      path:   self.file.as_ref().unwrap().path().to_path_buf(),
      cursor: self.cursor,
    });
    self.lsp.completions.clear_on_message = true;
    self.lsp.completions.tasks = tasks;
  }

  pub(crate) fn lsp_request_goto_definition(&mut self) {
    let task = self.lsp.client.send_first_capable(&command::GotoDefinition {
      path:   self.file.as_ref().unwrap().path().to_path_buf(),
      cursor: self.cursor,
    });

    self.lsp.goto_definition = task;
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
            types::Or2::A(completions) => completions,
            types::Or2::B(list) => list.items,
          });
        }
        self.lsp.completions.show = true;
        false
      } else {
        true
      }
    });

    if self.lsp.completions.show {
      let current_word = self.current_word_for_completions();

      Some(
        self
          .lsp
          .completions
          .completions
          .items
          .iter()
          .filter(|i| {
            let filter_text = i.filter_text.as_ref().unwrap_or(&i.label);
            // `starts_with` using a rope slice
            filter_text.bytes().take(current_word.byte_len()).eq(current_word.bytes())
          })
          .map(|i| i.label.clone())
          .collect(),
      )
    } else {
      None
    }
  }

  fn current_word_for_completions(&self) -> RopeSlice<'_> {
    let end = self.doc.cursor_offset(self.cursor);
    let len = self
      .doc
      .range(..end)
      .chars()
      .rev()
      .take_while(|c| c.is_alphanumeric())
      .map(|c| c.len_utf8())
      .sum::<usize>();

    self.doc.range(end - len..end)
  }
}

impl LspState {
  pub fn progress(&self) -> Vec<String> {
    let mut tasks = vec![];

    if self.save_task.is_some() {
      tasks.push("saving".to_string());
    }

    let now = std::time::Instant::now();

    self.client.servers(|state| {
      for progress in state.progress.values() {
        if progress
          .completed
          .is_none_or(|c| now.duration_since(c) < std::time::Duration::from_secs(5))
        {
          tasks.push(format!("{} {:3}%", progress.title, progress.progress * 100.0));
        }
      }
    });

    tasks
  }
}

impl CompletionsState {
  pub fn hide(&mut self) { self.show = false; }
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
