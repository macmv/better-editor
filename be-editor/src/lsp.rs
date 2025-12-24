use std::str::FromStr;

use be_lsp::{LspClient, types, types::Uri};
use be_task::Task;

use crate::{EditorState, filetype::FileType};

pub struct LspState {
  client:      LspClient,
  server_caps: types::ServerCapabilities,

  text_document:    Option<types::TextDocumentIdentifier>,
  document_version: i32,
  pub completions:  CompletionsState,
}

#[derive(Default)]
pub struct CompletionsState {
  task:        Option<Task<Option<types::CompletionResponse>>>,
  completions: Option<types::CompletionList>,
  show:        bool,
}

impl EditorState {
  pub(crate) fn connect_to_lsp(&mut self) {
    let Some(ft) = &self.filetype else { return };
    let Some(lsp) = lsp_for_ft(ft) else { return };

    let (client, server_caps) = LspClient::spawn(lsp);
    self.lsp = Some(LspState {
      client,
      server_caps,
      text_document: None,
      document_version: 0,
      completions: Default::default(),
    });

    let Some(lsp) = &mut self.lsp else { return };
    lsp.text_document = Some(types::TextDocumentIdentifier {
      uri: Uri::from_str(&format!(
        "file://{}",
        self.file.as_ref().unwrap().path().to_string_lossy()
      ))
      .unwrap(),
    });

    lsp.client.notify::<types::notification::DidOpenTextDocument>(
      types::DidOpenTextDocumentParams {
        text_document: types::TextDocumentItem {
          version:     0,
          uri:         lsp.text_document.clone().unwrap().uri.clone(),
          text:        self.doc.rope.to_string(),
          language_id: "rust".into(),
        },
      },
    );
  }

  pub(crate) fn lsp_notify_change(&mut self, change: crate::Change) {
    let range = types::Range {
      start: self.offset_to_lsp(change.range.start),
      end:   self.offset_to_lsp(change.range.end),
    };

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
  }

  pub(crate) fn lsp_request_completions(&mut self) {
    let cursor = self.cursor_to_lsp();

    let Some(lsp) = &mut self.lsp else { return };
    if lsp.server_caps.completion_provider.is_none() {
      return;
    }

    let Some(doc) = &lsp.text_document else { return };

    let task = lsp.client.request::<types::request::Completion>(types::CompletionParams {
      text_document_position:    types::TextDocumentPositionParams {
        text_document: doc.clone(),
        position:      cursor,
      },
      context:                   Some(types::CompletionContext {
        trigger_kind:      types::CompletionTriggerKind::INVOKED,
        trigger_character: None,
      }),
      work_done_progress_params: types::WorkDoneProgressParams::default(),
      partial_result_params:     types::PartialResultParams::default(),
    });

    lsp.completions.task = Some(task);
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

impl Drop for LspState {
  fn drop(&mut self) { self.client.shutdown(); }
}

fn lsp_for_ft(ft: &FileType) -> Option<&'static str> {
  match ft {
    FileType::Rust => Some("rust-analyzer"),
    _ => None,
  }
}
