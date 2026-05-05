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
      completion: Some(lsp::CompletionClientCapabilities {
        dynamic_registration: None,
        completion_item:      Some(lsp::CompletionItemCapabilities {
          // Snippts allow the client to fill in a portion (like a variable name).
          snippet_support:           None,
          // Commit characters fill in the completion when typed (like '.' or '(')
          commit_characters_support: None,
          // Allows the server to preselect a specific item.
          preselect_support:         None,
          // Lets the server specify exactly what edits to perform.
          insert_replace_support:    None,
          documentation_format:      Some(vec![
            lsp::MarkupKind::Markdown,
            lsp::MarkupKind::PlainText,
          ]),
          deprecated_support:        Some(true),
          // Adds a whole 'tagging' scheme, which in practice is just another way of saying
          // something is deprecated.
          tag_support:               None,
          resolve_support:           Some(lsp::ResolveSupportCapabilities {
            // rust-analyzer is picky, and needs `additionalTextEdits` for auto-imports to show up.
            //
            // TODO: Resolve support!
            properties: ["additionalTextEdits", "detail", "documentation"]
              .iter()
              .map(|s| s.to_string())
              .collect(),
          }),
          // If the server can pick the `insert_text_mode`.
          insert_text_mode_support:  None,
          label_details_support:     Some(true),
        }),
        // Has the client tell the server what triggered the completion.
        context_support:      Some(true),
        // Default insert mode (ie, how to handle whitespace) if not selected by the server.
        insert_text_mode:     None,
        completion_list:      Some(lsp::CompletionListCapabilities {
          // Default items to fill in. Not really used AFAICT.
          item_defaults: None,
        }),
        // All supported completion kinds. We support the default.
        completion_item_kind: None,
      }),
      definition: Some(lsp::DefinitionClientCapabilities { ..Default::default() }),
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
