pub fn client_capabilities() -> lsp::ClientCapabilities {
  lsp::ClientCapabilities {
    general: Some(lsp::GeneralClientCapabilities {
      position_encodings: Some(vec![
        lsp::PositionEncodingKind::Utf8,
        lsp::PositionEncodingKind::Utf16,
      ]),
      ..Default::default()
    }),
    text_document: Some(lsp::TextDocumentClientCapabilities {
      completion: Some(lsp::CompletionClientCapabilities { ..Default::default() }),
      formatting: Some(lsp::DocumentFormattingClientCapabilities { ..Default::default() }),
      publish_diagnostics: Some(lsp::PublishDiagnosticsClientCapabilities { ..Default::default() }),
      synchronization: Some(lsp::TextDocumentSyncClientCapabilities {
        did_save: Some(true),
        ..Default::default()
      }),
      ..Default::default()
    }),
    window: Some(lsp::WindowClientCapabilities {
      work_done_progress: Some(true),
      ..Default::default()
    }),
    ..Default::default()
  }
}
