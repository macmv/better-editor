use be_task::Task;
use parking_lot::Mutex;
use std::sync::{Arc, Weak};

mod client;
pub mod command;
mod init;

#[macro_use]
extern crate log;

pub extern crate lsp_types as types;

pub use client::LspClient;

pub struct LanguageServerStore {
  servers:    Vec<Arc<LanguageServerState>>,
  on_message: Arc<Mutex<Box<dyn Fn() + Send>>>,
}

#[derive(Default)]
pub struct LanguageClientState {
  servers: Vec<Weak<LanguageServerState>>,
}

pub struct LanguageServerState {
  client: Mutex<LspClient>,
  caps:   types::ServerCapabilities,
}

impl Default for LanguageServerStore {
  fn default() -> Self {
    LanguageServerStore { servers: vec![], on_message: Arc::new(Mutex::new(Box::new(|| {}))) }
  }
}

impl LanguageServerStore {
  pub fn set_on_message<F: Fn() + Send + 'static>(&mut self, f: F) {
    *self.on_message.lock() = Box::new(f);
  }

  pub fn spawn(&mut self, cmd: &str) -> Weak<LanguageServerState> {
    let (client, server_caps) = LspClient::spawn(cmd, self.on_message.clone());

    let state = Arc::new(LanguageServerState { client: Mutex::new(client), caps: server_caps });
    let weak = Arc::downgrade(&state);
    self.servers.push(state);

    weak
  }
}

impl LanguageClientState {
  pub fn add(&mut self, server: Weak<LanguageServerState>) { self.servers.push(server); }

  pub fn send<T: command::LspCommand>(&mut self, command: &T) -> Vec<Task<T::Result>> {
    let mut tasks = vec![];

    for server in &self.servers {
      if let Some(server) = server.upgrade() {
        if let Some(task) = command.send(&mut server.client.lock()) {
          tasks.push(task);
        }
      }
    }

    tasks
  }
}
