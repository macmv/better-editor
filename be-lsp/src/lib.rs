use std::collections::HashMap;

mod client;
mod init;

#[macro_use]
extern crate log;

pub extern crate lsp_types as types;

pub use client::LspClient;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct LanguageServerId(u32);

pub struct LanguageServerStore {
  servers:        HashMap<LanguageServerId, LanguageServerState>,
  next_server_id: LanguageServerId,
}

pub struct LanguageServerState {
  client: LspClient,
  caps:   types::ServerCapabilities,
}

impl Default for LanguageServerStore {
  fn default() -> Self {
    LanguageServerStore { servers: HashMap::new(), next_server_id: LanguageServerId(0) }
  }
}

impl LanguageServerStore {
  pub fn spawn(&mut self, cmd: &str) -> LanguageServerId {
    let id = self.next_server_id;
    self.next_server_id.0 += 1;
    let (client, server_caps) = LspClient::spawn(cmd);

    self.servers.insert(id, LanguageServerState { client, caps: server_caps });

    id
  }
}
