use std::{
  cell::RefCell,
  collections::HashMap,
  io,
  path::{Path, PathBuf},
  rc::Rc,
  sync::Arc,
};

use be_config::Config;
use be_editor::{EditorEvent, EditorState};
use be_git::Repo;
use be_lsp::LanguageServerStore;
use be_shared::{SharedHandle, WeakHandle};
use parking_lot::Mutex;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EditorId(u32);

pub struct Workspace {
  pub root: PathBuf,

  pub config:  Rc<RefCell<Config>>,
  pub editors: HashMap<EditorId, WeakHandle<EditorState>>,
  pub repo:    Rc<RefCell<Option<Repo>>>,
  pub lsp:     Rc<RefCell<LanguageServerStore>>,

  next_id:  EditorId,
  notifier: Arc<Mutex<Box<dyn Fn(WorkspaceEvent) + Send>>>,

  editors_by_path: HashMap<PathBuf, WeakHandle<EditorState>>,
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

    let root = std::env::current_dir().unwrap();
    let repo = Repo::open(&root);

    Workspace {
      root,
      config,
      editors: HashMap::new(),
      repo: Rc::new(RefCell::new(Some(repo))),
      lsp: Rc::new(RefCell::new(lsp)),

      next_id: EditorId(0),
      notifier,

      editors_by_path: HashMap::new(),
    }
  }

  pub fn new_editor(&mut self) -> SharedHandle<EditorState> {
    let mut editor = EditorState::from("💖hello\n💖foobar\nsdjkhfl\nworld\n");

    editor.repo = self.repo.clone();
    editor.config = self.config.clone();
    editor.lsp.store = self.lsp.clone();
    editor.send = Some(Box::new({
      let notifier = self.notifier.clone();
      move |ev| (notifier.lock())(WorkspaceEvent::Editor(ev))
    }));

    let handle = SharedHandle::new(editor);

    let id = self.next_id;
    self.editors.insert(id, SharedHandle::downgrade(&handle));
    self.next_id.0 += 1;
    handle
  }

  pub fn open_file(&mut self, path: &Path) -> io::Result<SharedHandle<EditorState>> {
    let canon = path.canonicalize()?;

    if let Some(handle) = self.editors_by_path.get(&canon)
      && let Some(handle) = handle.upgrade()
    {
      Ok(handle)
    } else {
      let mut editor = self.new_editor();
      editor.open(&canon)?;
      self.editors_by_path.insert(canon, SharedHandle::downgrade(&editor));
      Ok(editor)
    }
  }

  pub fn set_waker(&self, wake: impl Fn(WorkspaceEvent) + Send + 'static) {
    *self.notifier.lock() = Box::new(wake);
  }

  pub fn cleanup_editors(&mut self) { self.editors.retain(|_, v| v.can_upgrade()); }
}
