use be_lsp::{LspClient, types};

use crate::{EditorState, filetype::FileType};

pub struct LspState {
  client:      LspClient,
  server_caps: types::ServerCapabilities,
}

impl EditorState {
  pub fn connect_to_lsp(&mut self) {
    let Some(ft) = &self.filetype else { return };
    let Some(lsp) = lsp_for_ft(ft) else { return };

    let (client, server_caps) = LspClient::spawn(lsp);
    self.lsp = Some(LspState { client, server_caps });
  }
}

fn lsp_for_ft(ft: &FileType) -> Option<&'static str> {
  match ft {
    FileType::Rust => Some("rust-analyzer"),
    _ => None,
  }
}
