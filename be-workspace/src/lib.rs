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
use be_fs::{WorkspaceRoot, WorkspaceWatcher};
use be_git::Repo;
use be_input::Clipboard;
use be_lsp::LanguageServerStore;
use be_shared::{SharedHandle, WeakHandle};
use parking_lot::Mutex;

pub struct Workspace {
  pub root: WorkspaceRoot,

  pub fs:        WorkspaceWatcher,
  pub config:    Rc<RefCell<Config>>,
  pub repo:      SharedHandle<Option<Repo>>,
  pub lsp:       Rc<RefCell<LanguageServerStore>>,
  pub clipboard: SharedHandle<Clipboard>,

  notifier: Arc<Mutex<Box<dyn Fn(WorkspaceEvent) + Send>>>,

  next_editor:     u32,
  editors:         HashMap<u32, SharedHandle<EditorState>>,
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

    let root = WorkspaceRoot::from_path(std::env::current_dir().unwrap());
    let repo = Repo::open(&root.as_path());
    let fs = {
      let waker = notifier.clone();
      WorkspaceWatcher::new(&root, move || {
        (waker.lock())(WorkspaceEvent::Refresh);
      })
    };

    Workspace {
      root,
      config,
      fs,
      repo: SharedHandle::new(Some(repo)),
      lsp: Rc::new(RefCell::new(lsp)),
      clipboard: SharedHandle::new(Clipboard::dummy()),

      notifier,

      next_editor: 0,
      editors: HashMap::new(),
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
    editor.clipboard = self.clipboard.clone();

    let handle = SharedHandle::new(editor);

    let id = self.next_editor;
    self.editors.insert(id, handle.clone());
    self.next_editor += 1;
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

  pub fn editors(&self) -> impl Iterator<Item = &SharedHandle<EditorState>> {
    self.editors.values()
  }
  pub fn editors_mut(&mut self) -> impl Iterator<Item = &mut SharedHandle<EditorState>> {
    self.editors.values_mut()
  }
}
