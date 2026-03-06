use std::path::{Path, PathBuf};

use relative_path::{RelativePath, RelativePathBuf};

/// The root path of a workspace. This is non-portable, and is only usable on
/// the machine hosting a session.
#[derive(Debug, Clone)]
pub struct WorkspaceRoot {
  path: PathBuf,
}

/// A path to a file within a workspace. This can only be resolved to a path
/// using a [`WorkspaceRoot`]. It is portable and can be consistently converted
/// to/from it's owned counterpart, [`WorkspacePathBuf`].
#[derive(Debug)]
pub struct WorkspacePath {
  path: RelativePath,
}

/// A path to a file within a workspace. This can only be resolved to a path
/// using a [`WorkspaceRoot`]. It is portable and can safely be sent over the
/// network.
#[derive(Debug, Clone)]
pub struct WorkspacePathBuf {
  path: RelativePathBuf,
}

impl WorkspaceRoot {
  pub fn from_path(path: PathBuf) -> Self { WorkspaceRoot { path } }

  pub fn as_path(&self) -> &Path { &self.path }
}
