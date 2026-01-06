pub fn client_capabilities() -> lsp_types::ClientCapabilities {
  lsp_types::ClientCapabilities {
    general: Some(lsp_types::GeneralClientCapabilities {
      position_encodings: Some(vec![
        lsp_types::PositionEncodingKind::UTF8,
        lsp_types::PositionEncodingKind::UTF16,
      ]),
      ..Default::default()
    }),
    text_document: Some(lsp_types::TextDocumentClientCapabilities {
      completion: Some(lsp_types::CompletionClientCapabilities { ..Default::default() }),
      formatting: Some(lsp_types::DocumentFormattingClientCapabilities { ..Default::default() }),
      ..Default::default()
    }),
    ..Default::default()
  }
}
