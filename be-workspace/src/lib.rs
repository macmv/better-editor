use std::{
  collections::HashMap,
  ops::{Deref, DerefMut},
  path::PathBuf,
};

use be_editor::EditorState;
use be_git::Repo;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EditorId(u32);

pub struct Workspace {
  pub root:    PathBuf,
  pub editors: HashMap<EditorId, EditorState>,
  pub repo:    Option<Repo>,
}

pub struct WorkspaceEditor<'a> {
  workspace: &'a mut Workspace,
  id:        EditorId,
}

impl Workspace {
  pub fn new() -> Self {
    Workspace { root: std::env::current_dir().unwrap(), editors: HashMap::new(), repo: None }
  }

  pub fn editor(&mut self, id: EditorId) -> WorkspaceEditor<'_> {
    WorkspaceEditor { workspace: self, id }
  }
}

impl Deref for WorkspaceEditor<'_> {
  type Target = EditorState;

  fn deref(&self) -> &Self::Target { &self.workspace.editors[&self.id] }
}

impl DerefMut for WorkspaceEditor<'_> {
  fn deref_mut(&mut self) -> &mut Self::Target { self.workspace.editors.get_mut(&self.id).unwrap() }
}
