use std::convert::Infallible;

use be_task::Task;
use types::Uri;

use crate::LspClient;

pub trait LspCommand {
  type Result;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool;
  fn send(&self, client: &mut LspClient) -> Option<Task<Self::Result>>;
}

pub struct DidOpenTextDocument {
  pub uri:         Uri,
  pub text:        String,
  pub language_id: String,
}

pub struct Completion {
  pub uri: Uri,
}

impl LspCommand for DidOpenTextDocument {
  type Result = Infallible;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Infallible>> {
    client.notify::<types::notification::DidOpenTextDocument>(types::DidOpenTextDocumentParams {
      text_document: types::TextDocumentItem {
        version:     0,
        uri:         self.uri.clone(),
        text:        self.text.clone(),
        language_id: self.language_id.clone(),
      },
    });

    None
  }
}

impl LspCommand for Completion {
  type Result = Option<types::CompletionResponse>;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.completion_provider.is_some()
  }

  fn send(&self, client: &mut LspClient) -> Option<Task<Option<types::CompletionResponse>>> {
    Some(client.request::<types::request::Completion>(types::CompletionParams {
      text_document_position:    types::TextDocumentPositionParams {
        text_document: types::TextDocumentIdentifier { uri: self.uri.clone() },
        position:      types::Position { line: 0, character: 0 },
      },
      context:                   None,
      work_done_progress_params: types::WorkDoneProgressParams::default(),
      partial_result_params:     types::PartialResultParams::default(),
    }))
  }
}
