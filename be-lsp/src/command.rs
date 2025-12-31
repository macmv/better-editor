use types::Uri;

use crate::LspClient;

pub trait LspCommand {
  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool;
  fn send(self, client: &mut LspClient);
}

pub struct DidOpenTextDocument {
  pub uri:         Uri,
  pub text:        String,
  pub language_id: String,
}

impl LspCommand for DidOpenTextDocument {
  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool {
    caps.text_document_sync.is_some()
  }

  fn send(self, client: &mut LspClient) {
    client.notify::<types::notification::DidOpenTextDocument>(types::DidOpenTextDocumentParams {
      text_document: types::TextDocumentItem {
        version:     0,
        uri:         self.uri,
        text:        self.text,
        language_id: self.language_id,
      },
    });
  }
}
