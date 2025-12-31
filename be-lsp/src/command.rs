use std::{convert::Infallible, path::PathBuf, str::FromStr};

use be_task::Task;
use types::Uri;

use crate::LspClient;

pub trait LspCommand {
  type Result;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool;
  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>>;
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
    if !client.state.opened_files.insert(self.path.clone()) {
      return None;
    }

    client.notify::<types::notification::DidOpenTextDocument>(types::DidOpenTextDocumentParams {
      text_document: types::TextDocumentItem {
        version:     0,
        uri:         Uri::from_str(&format!("file://{}", self.path.to_string_lossy())).unwrap(),
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
    if !client.state.opened_files.insert(self.path.clone()) {
      error!("cannot change a file that is not opened: {}", self.path.display());
      return None;
    }

    client.notify::<types::notification::DidChangeTextDocument>(
      types::DidChangeTextDocumentParams {
        text_document:   types::VersionedTextDocumentIdentifier {
          uri:     Uri::from_str(&format!("file://{}", self.path.to_string_lossy())).unwrap(),
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
        text_document: types::TextDocumentIdentifier {
          uri: Uri::from_str(&format!("file://{}", self.path.to_string_lossy())).unwrap(),
        },
        position:      self.cursor,
      },
      context:                   None,
      work_done_progress_params: types::WorkDoneProgressParams::default(),
      partial_result_params:     types::PartialResultParams::default(),
    }))
  }
}
