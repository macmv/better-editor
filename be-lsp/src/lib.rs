use be_config::LanguageName;
use be_task::Task;
use parking_lot::Mutex;
use std::{
  collections::HashMap,
  ops::Range,
  sync::{Arc, Weak},
};

mod client;
pub mod command;
mod init;

#[macro_use]
extern crate log;

pub extern crate lsp as types;

pub use client::LspClient;

use crate::client::LspState;

pub struct LanguageServerStore {
  servers:    HashMap<LanguageServerKey, Arc<LanguageServerState>>,
  on_message: Arc<Mutex<Box<dyn Fn() + Send>>>,
}

pub struct Diagnostic {
  pub range:    Range<usize>,
  pub message:  String,
  pub severity: Option<types::DiagnosticSeverity>,
}

pub struct TextEdit {
  pub range:    Range<usize>,
  pub new_text: String,
}

pub struct Progress {
  pub title:     String,
  pub message:   Option<String>,
  pub progress:  f64,
  pub completed: Option<std::time::Instant>,
}

#[derive(Clone, Hash, Eq, PartialEq)]
pub enum LanguageServerKey {
  /// The language server for a particular language. One will be spawned when a
  /// project for a given language is opened.
  ///
  /// TODO: Shared language keys?
  Language(LanguageName),
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

  pub fn send_first_capable<T: command::LspCommand>(
    &mut self,
    command: &T,
  ) -> Option<Task<T::Result>> {
    for server in self.servers.values().filter_map(|s| s.upgrade()) {
      if command.is_capable(&server.caps) {
        if let Some(t) = command.send(&mut server.client.lock()) {
          return Some(t);
        } else {
          // This function shouldn't be called for notifications.
          warn!("no task returned for `send_first_capable`");
          return None;
        }
      }
    }

    None
  }
}

impl Drop for LanguageServerStore {
  fn drop(&mut self) {
    for (_, server) in self.servers.drain() {
      let server = Arc::into_inner(server).expect("server should not have multiple references");
      server.client.into_inner().shutdown();
    }
  }
}
