pub fn client_capabilities() -> lsp_types::ClientCapabilities {
  lsp_types::ClientCapabilities {
    text_document: Some(lsp_types::TextDocumentClientCapabilities {
      completion: Some(lsp_types::CompletionClientCapabilities { ..Default::default() }),
      formatting: Some(lsp_types::DocumentFormattingClientCapabilities { ..Default::default() }),
      ..Default::default()
    }),
    ..Default::default()
  }
}
