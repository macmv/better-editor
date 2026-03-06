use std::{
  fmt,
  ops::Deref,
  path::{Path, PathBuf},
};

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
#[derive(PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct WorkspacePath {
  path: RelativePath,
}

/// A path to a file within a workspace. This can only be resolved to a path
/// using a [`WorkspaceRoot`]. It is portable and can safely be sent over the
/// network.
#[derive(Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct WorkspacePathBuf {
  path: RelativePathBuf,
}

impl WorkspaceRoot {
  pub fn from_path(path: PathBuf) -> Self { WorkspaceRoot { path } }

  pub fn as_path(&self) -> &Path { &self.path }
}

impl Deref for WorkspacePathBuf {
  type Target = WorkspacePath;

  fn deref(&self) -> &Self::Target { WorkspacePath::new(&self.path) }
}

impl<'a, T: ?Sized + AsRef<str>> From<&'a T> for WorkspacePathBuf {
  #[inline]
  fn from(path: &'a T) -> Self { WorkspacePathBuf { path: RelativePathBuf::from(path) } }
}

impl WorkspacePath {
  pub fn new<S>(s: &S) -> &WorkspacePath
  where
    S: AsRef<str> + ?Sized,
  {
    // SAFETY: `WorkspacePath` is #[repr(transparent)], and so is `RelativePath`.
    unsafe { &*(s.as_ref() as *const str as *const WorkspacePath) }
  }

  pub fn starts_with(&self, other: &WorkspacePath) -> bool { self.path.starts_with(&other.path) }
}

impl fmt::Debug for WorkspacePath {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.path.fmt(f) }
}

impl fmt::Display for WorkspacePath {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.path.fmt(f) }
}

impl fmt::Debug for WorkspacePathBuf {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.path.fmt(f) }
}

impl fmt::Display for WorkspacePathBuf {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.path.fmt(f) }
}
