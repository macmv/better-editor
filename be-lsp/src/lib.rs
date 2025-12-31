use std::sync::{Arc, Weak};

mod client;
mod init;

#[macro_use]
extern crate log;

pub extern crate lsp_types as types;

pub use client::LspClient;

pub struct LanguageServerStore {
  servers: Vec<Arc<LanguageServerState>>,
}

#[derive(Default)]
pub struct LanguageClientState {
  servers: Vec<Weak<LanguageServerState>>,
}

pub struct LanguageServerState {
  client: LspClient,
  caps:   types::ServerCapabilities,
}

impl Default for LanguageServerStore {
  fn default() -> Self { LanguageServerStore { servers: vec![] } }
}

impl LanguageServerStore {
  pub fn spawn(&mut self, cmd: &str) -> Weak<LanguageServerState> {
    let (client, server_caps) = LspClient::spawn(cmd);

    let state = Arc::new(LanguageServerState { client, caps: server_caps });
    let weak = Arc::downgrade(&state);
    self.servers.push(state);

    weak
  }
}

pub trait LspCommand {
  type Request: types::request::Request;

  fn is_capable(&self, caps: &types::ServerCapabilities) -> bool;
  fn params(&self) -> <Self::Request as types::request::Request>::Params;
}

impl LanguageClientState {
  pub fn spawn(&mut self, cmd: &str) {
    // TODO
  }

  pub fn notify<T: LspCommand>(&mut self, command: &T) {
    for server in &self.servers {
      if let Some(server) = server.upgrade() {
        // server.client.request::<T::Request>(command.params());
      }
    }
  }
}
