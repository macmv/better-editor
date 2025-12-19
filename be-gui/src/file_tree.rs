use std::path::PathBuf;

pub struct FileTree {
  root: PathBuf,
}

impl FileTree {
  pub fn new() -> Self { FileTree { root: PathBuf::new() } }
}
