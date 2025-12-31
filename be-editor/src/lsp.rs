use std::{cell::RefCell, rc::Rc, str::FromStr};

use be_lsp::{
  LanguageClientState, command,
  types::{self, Uri},
};
use be_task::Task;

use crate::EditorState;

#[derive(Default)]
pub struct LspState {
  pub store:  Rc<RefCell<be_lsp::LanguageServerStore>>,
  pub client: LanguageClientState,

  text_document:    Option<types::TextDocumentIdentifier>,
  document_version: i32,
  pub completions:  CompletionsState,

  // FIXME: ew.
  pub set_waker: bool,
}

#[derive(Default)]
pub struct CompletionsState {
  task:        Vec<Task<Option<types::CompletionResponse>>>,
  completions: Option<types::CompletionList>,
  show:        bool,
}

impl EditorState {
  pub(crate) fn connect_to_lsp(&mut self) {
    let Some(ft) = &self.filetype else { return };
    let config = self.config.borrow();
    let Some(language) = config.language.get(ft.name()) else { return };
    let Some(lsp) = &language.lsp else { return };

    let server = self.lsp.store.borrow_mut().spawn(&lsp.command);
    self.lsp.client.add(server);

    self.lsp.text_document = Some(types::TextDocumentIdentifier {
      uri: Uri::from_str(&format!(
        "file://{}",
        self.file.as_ref().unwrap().path().to_string_lossy()
      ))
      .unwrap(),
    });

    self.lsp.client.send(&command::DidOpenTextDocument {
      uri:         self.lsp.text_document.clone().unwrap().uri.clone(),
      text:        self.doc.rope.to_string(),
      language_id: "rust".into(),
    });
  }

  pub(crate) fn lsp_notify_change(&mut self, change: crate::Change) {
    let range = types::Range {
      start: self.offset_to_lsp(change.range.start),
      end:   self.offset_to_lsp(change.range.end),
    };

    /*
    let Some(lsp) = &mut self.lsp else { return };
    let Some(doc) = &lsp.text_document else { return };

    lsp.document_version += 1;

    lsp.client.notify::<types::notification::DidChangeTextDocument>(
      types::DidChangeTextDocumentParams {
        text_document:   types::VersionedTextDocumentIdentifier {
          uri:     doc.uri.clone(),
          version: lsp.document_version,
        },
        content_changes: vec![types::TextDocumentContentChangeEvent {
          range:        Some(range),
          range_length: None,
          text:         change.text,
        }],
      },
    );
    */
  }

  pub(crate) fn lsp_request_completions(&mut self) {
    let cursor = self.cursor_to_lsp();

    let Some(doc) = &self.lsp.text_document else { return };

    let tasks = self.lsp.client.send(&command::Completion { uri: doc.uri.clone() });
    self.lsp.completions.task = tasks;
  }

  pub fn completions(&mut self) -> Option<Vec<String>> {
    /*
    let Some(lsp) = &mut self.lsp else { return None };

    if let Some(completed) = lsp.completions.task.as_mut().and_then(|task| task.completed()) {
      lsp.completions.task = None;
      lsp.completions.completions = completed.map(|res| match res {
        types::CompletionResponse::List(list) => list,
        types::CompletionResponse::Array(completions) => {
          types::CompletionList { is_incomplete: false, items: completions }
        }
      });
      lsp.completions.show = true;
    }

    if lsp.completions.show {
      Some(
        lsp
          .completions
          .completions
          .as_ref()
          .unwrap()
          .items
          .iter()
          .map(|i| i.label.clone())
          .collect(),
      )
    } else {
      None
    }
    */
    None
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
