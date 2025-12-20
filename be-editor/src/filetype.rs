use std::fmt;

use crate::EditorState;

#[derive(Clone, Copy)]
pub enum FileType {
  Rust,
  Toml,
  Markdown,
}

impl fmt::Display for FileType {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.name()) }
}

impl FileType {
  fn name(&self) -> &'static str {
    match self {
      FileType::Rust => "rust",
      FileType::Toml => "toml",
      FileType::Markdown => "markdown",
    }
  }
}

impl EditorState {
  pub(crate) fn detect_filetype(&mut self) {
    let Some(file) = &self.file else { return };

    self.filetype = match file.path().extension().and_then(|e| e.to_str()) {
      Some("rs") => Some(FileType::Rust),
      Some("md") => Some(FileType::Markdown),
      Some("toml") => Some(FileType::Toml),

      _ => None,
    }
  }
}
