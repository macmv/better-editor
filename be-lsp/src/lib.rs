use be_task::Task;
use parking_lot::Mutex;
use std::{
  collections::HashMap,
  sync::{Arc, Weak},
};

mod client;
pub mod command;
mod init;

#[macro_use]
extern crate log;

pub extern crate lsp_types as types;

pub use client::LspClient;

use crate::client::LspState;

pub struct LanguageServerStore {
  servers:    HashMap<LanguageServerKey, Arc<LanguageServerState>>,
  on_message: Arc<Mutex<Box<dyn Fn() + Send>>>,
}

#[derive(Clone, Hash, Eq, PartialEq)]
pub enum LanguageServerKey {
  /// The language server for a particular language. One will be spawned when a
  /// project for a given language is opened.
  ///
  /// TODO: Shared language keys?
  Language(String),
}

#[derive(Default)]
pub struct LanguageClientState {
  servers: HashMap<LanguageServerKey, Weak<LanguageServerState>>,
}

pub struct LanguageServerState {
  client: Mutex<LspClient>,
  caps:   types::ServerCapabilities,
}

impl Default for LanguageServerStore {
  fn default() -> Self {
    LanguageServerStore {
      servers:    HashMap::new(),
      on_message: Arc::new(Mutex::new(Box::new(|| {}))),
    }
  }
}

impl LanguageServerStore {
  pub fn set_on_message<F: Fn() + Send + 'static>(&mut self, f: F) {
    *self.on_message.lock() = Box::new(f);
  }

  pub fn get(&self, key: &LanguageServerKey) -> Option<Weak<LanguageServerState>> {
    self.servers.get(key).map(Arc::downgrade)
  }

  pub fn spawn(&mut self, key: LanguageServerKey, cmd: &str) -> Weak<LanguageServerState> {
    let (client, server_caps) = LspClient::spawn(cmd, self.on_message.clone());

    let state = Arc::new(LanguageServerState { client: Mutex::new(client), caps: server_caps });
    let weak = Arc::downgrade(&state);
    self.servers.insert(key, state);

    weak
  }
}

impl LanguageClientState {
  pub fn set(&mut self, key: LanguageServerKey, server: Weak<LanguageServerState>) {
    self.servers.insert(key, server);
  }

  pub fn servers(&self, mut f: impl FnMut(&LspState)) {
    for server in self.servers.values().filter_map(|s| s.upgrade()) {
      f(&server.client.lock().state.lock());
    }
  }

  pub fn send<T: command::LspCommand>(&mut self, command: &T) -> Vec<Task<T::Result>> {
    let mut tasks = vec![];

    self.servers.retain(|_, server| {
      if let Some(server) = server.upgrade() {
        if !command.is_capable(&server.caps) {
          return true;
        }

        if let Some(task) = command.send(&mut server.client.lock()) {
          tasks.push(task);
        }
        true
      } else {
        false
      }
    });

    tasks
  }
}
