use std::path::PathBuf;

use relative_path::{RelativePath, RelativePathBuf};

/// The root path of a workspace. This is non-portable, and is only usable on
/// the machine hosting a session.
pub struct WorkspaceRoot {
  path: PathBuf,
}

/// A path to a file within a workspace. This can only be resolved to a path
/// using a [`WorkspaceRoot`]. It is portable and can be consistently converted
/// to/from it's owned counterpart, [`WorkspacePathBuf`].
pub struct WorkspacePath {
  path: RelativePath,
}

/// A path to a file within a workspace. This can only be resolved to a path
/// using a [`WorkspaceRoot`]. It is portable and can safely be sent over the
/// network.
pub struct WorkspacePathBuf {
  path: RelativePathBuf,
}
