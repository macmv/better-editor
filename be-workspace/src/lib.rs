use std::{
  cell::RefCell,
  collections::HashMap,
  ops::{Deref, DerefMut},
  path::PathBuf,
  rc::Rc,
  sync::Arc,
};

use be_editor::EditorState;
use be_git::Repo;
use be_lsp::LanguageServerStore;
use parking_lot::Mutex;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EditorId(u32);

pub struct Workspace {
  pub root: PathBuf,

  pub editors: HashMap<EditorId, EditorState>,
  pub repo:    Option<Repo>,
  pub lsp:     Rc<RefCell<LanguageServerStore>>,

  waker: Arc<Mutex<Box<dyn Fn() + Send>>>,
}

pub struct WorkspaceEditor<'a> {
  workspace: &'a mut Workspace,
  id:        EditorId,
}

impl Workspace {
  pub fn new() -> Self {
    let waker: Arc<Mutex<Box<dyn Fn() + Send>>> = Arc::new(Mutex::new(Box::new(|| {})));

    let mut lsp = LanguageServerStore::default();
    lsp.set_on_message(waker.clone());

    Workspace {
      root: std::env::current_dir().unwrap(),
      editors: HashMap::new(),
      repo: None,
      lsp: Rc::new(RefCell::new(lsp)),

      waker,
    }
  }

  pub fn editor(&mut self, id: EditorId) -> WorkspaceEditor<'_> {
    WorkspaceEditor { workspace: self, id }
  }

  pub fn set_waker(&self, wake: impl Fn() + Send + 'static) { *self.waker.lock() = Box::new(wake); }
}

impl Deref for WorkspaceEditor<'_> {
  type Target = EditorState;

  fn deref(&self) -> &Self::Target { &self.workspace.editors[&self.id] }
}

impl DerefMut for WorkspaceEditor<'_> {
  fn deref_mut(&mut self) -> &mut Self::Target { self.workspace.editors.get_mut(&self.id).unwrap() }
}
