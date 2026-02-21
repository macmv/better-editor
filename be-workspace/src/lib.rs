use std::{
  cell::RefCell,
  collections::HashMap,
  ops::{Deref, DerefMut},
  path::PathBuf,
  rc::Rc,
  sync::Arc,
};

use be_config::Config;
use be_editor::{EditorEvent, EditorState};
use be_git::Repo;
use be_lsp::LanguageServerStore;
use parking_lot::Mutex;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EditorId(u32);

pub struct Workspace {
  pub root: PathBuf,

  pub config:  Rc<RefCell<Config>>,
  pub editors: HashMap<EditorId, EditorState>,
  pub repo:    Option<Repo>,
  pub lsp:     Rc<RefCell<LanguageServerStore>>,

  next_id:  EditorId,
  notifier: Arc<Mutex<Box<dyn Fn(WorkspaceEvent) + Send>>>,
}

pub struct WorkspaceEditor<'a> {
  workspace: &'a mut Workspace,
  id:        EditorId,
}

#[derive(Debug)]
pub enum WorkspaceEvent {
  Refresh,
  Editor(EditorEvent),
}

impl Workspace {
  pub fn new(config: Rc<RefCell<Config>>) -> Self {
    let notifier: Arc<Mutex<Box<dyn Fn(WorkspaceEvent) + Send>>> =
      Arc::new(Mutex::new(Box::new(|_| {})));

    let mut lsp = LanguageServerStore::default();
    {
      let waker = notifier.clone();
      lsp.set_on_message(Arc::new(Mutex::new(Box::new(move || {
        (waker.lock())(WorkspaceEvent::Refresh);
      }))));
    }

    Workspace {
      root: std::env::current_dir().unwrap(),
      config,
      editors: HashMap::new(),
      repo: None,
      lsp: Rc::new(RefCell::new(lsp)),

      next_id: EditorId(0),
      notifier,
    }
  }

  pub fn new_editor(&mut self) -> EditorId {
    let mut editor = EditorState::from("💖hello\n💖foobar\nsdjkhfl\nworld\n");

    editor.config = self.config.clone();
    editor.lsp.store = self.lsp.clone();
    editor.send = Some(Box::new({
      let notifier = self.notifier.clone();
      move |ev| (notifier.lock())(WorkspaceEvent::Editor(ev))
    }));

    let id = self.next_id;
    self.editors.insert(id, editor);
    self.next_id.0 += 1;
    id
  }

  pub fn editor(&mut self, id: EditorId) -> WorkspaceEditor<'_> {
    WorkspaceEditor { workspace: self, id }
  }

  pub fn set_waker(&self, wake: impl Fn(WorkspaceEvent) + Send + 'static) {
    *self.notifier.lock() = Box::new(wake);
  }
}

impl Deref for WorkspaceEditor<'_> {
  type Target = EditorState;

  fn deref(&self) -> &Self::Target { &self.workspace.editors[&self.id] }
}

impl DerefMut for WorkspaceEditor<'_> {
  fn deref_mut(&mut self) -> &mut Self::Target { self.workspace.editors.get_mut(&self.id).unwrap() }
}
