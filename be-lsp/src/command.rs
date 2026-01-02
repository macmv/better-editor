use std::{
  convert::Infallible,
  path::{Path, PathBuf},
  str::FromStr,
};

use be_task::Task;
use serde_json::value::RawValue;
use types::Uri;

use crate::{LspClient, client::LspWorker};

pub trait LspCommand {
  type Result;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool;
  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>>;
}

fn doc_uri(path: &Path) -> Uri {
  Uri::from_str(&format!("file://{}", path.to_string_lossy())).unwrap()
}

fn doc_id(path: &Path) -> types::TextDocumentIdentifier {
  types::TextDocumentIdentifier { uri: doc_uri(path) }
}

pub struct DidOpenTextDocument {
  pub path:        PathBuf,
  pub text:        String,
  pub language_id: String,
}

impl LspCommand for DidOpenTextDocument {
  type Result = Infallible;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Infallible>> {
    if !client.state.lock().opened_files.insert(self.path.clone()) {
      return None;
    }

    client.notify::<types::notification::DidOpenTextDocument>(types::DidOpenTextDocumentParams {
      text_document: types::TextDocumentItem {
        version:     0,
        uri:         doc_uri(&self.path),
        text:        self.text.clone(),
        language_id: self.language_id.clone(),
      },
    });

    None
  }
}

pub struct DidChangeTextDocument {
  pub path:    PathBuf,
  pub version: i32,
  pub changes: Vec<(types::Range, String)>,
}

impl LspCommand for DidChangeTextDocument {
  type Result = Infallible;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>> {
    if !client.state.lock().opened_files.contains(&self.path) {
      error!("cannot change a file that is not opened: {}", self.path.display());
      return None;
    }

    client.notify::<types::notification::DidChangeTextDocument>(
      types::DidChangeTextDocumentParams {
        text_document:   types::VersionedTextDocumentIdentifier {
          uri:     doc_uri(&self.path),
          version: self.version,
        },
        content_changes: self
          .changes
          .iter()
          .map(|change| types::TextDocumentContentChangeEvent {
            range:        Some(change.0),
            range_length: None,
            text:         change.1.clone(),
          })
          .collect(),
      },
    );

    None
  }
}

pub struct Completion {
  pub path:   PathBuf,
  pub cursor: types::Position,
}

impl LspCommand for Completion {
  type Result = Option<types::CompletionResponse>;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.completion_provider.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Option<types::CompletionResponse>>> {
    Some(client.request::<types::request::Completion>(types::CompletionParams {
      text_document_position:    types::TextDocumentPositionParams {
        text_document: doc_id(&self.path),
        position:      self.cursor,
      },
      context:                   None,
      work_done_progress_params: types::WorkDoneProgressParams::default(),
      partial_result_params:     types::PartialResultParams::default(),
    }))
  }
}

pub struct DocumentFormat {
  pub path: PathBuf,
}

impl LspCommand for DocumentFormat {
  type Result = Option<Vec<types::TextEdit>>;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.document_formatting_provider.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Option<Vec<types::TextEdit>>>> {
    Some(client.request::<types::request::Formatting>(types::DocumentFormattingParams {
      text_document:             doc_id(&self.path),
      options:                   types::FormattingOptions {
        tab_size: 2,
        insert_spaces: true,
        ..Default::default()
      },
      work_done_progress_params: types::WorkDoneProgressParams::default(),
    }))
  }
}

impl LspWorker {
  pub fn handle_notification(&self, method: &str, params: Option<Box<RawValue>>) {
    if method == "textDocument/publishDiagnostics" {
      let params =
        serde_json::from_str::<lsp_types::PublishDiagnosticsParams>(params.unwrap().get()).unwrap();

      let path = PathBuf::from(params.uri.path().as_str());
      self.state.lock().diagnostics.insert(path, params.diagnostics);
    } else {
      info!("unhandled notification: {}", method);
    }
  }
}
