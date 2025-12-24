use std::str::FromStr;

use be_lsp::{
  LspClient,
  types::{self, Uri},
};

use crate::{EditorState, filetype::FileType};

pub struct LspState {
  client:      LspClient,
  server_caps: types::ServerCapabilities,

  text_document:    Option<types::TextDocumentIdentifier>,
  document_version: i32,
}

impl EditorState {
  pub(crate) fn connect_to_lsp(&mut self) {
    let Some(ft) = &self.filetype else { return };
    let Some(lsp) = lsp_for_ft(ft) else { return };

    let (client, server_caps) = LspClient::spawn(lsp);
    self.lsp = Some(LspState { client, server_caps, text_document: None, document_version: 0 });

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
          range:        None,
          range_length: None,
          text:         change.text,
        }],
      },
    );
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
