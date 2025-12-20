use std::path::PathBuf;

use crate::filetype::FileType;

pub fn load_grammar(ft: &FileType) {
  if let Some(repo) = repo(ft) {
    let language_path = PathBuf::new()
      .join(std::env::home_dir().unwrap())
      .join(".local")
      .join("share")
      .join("be")
      .join("language")
      .join(ft.name());

    std::fs::create_dir_all(&language_path).unwrap();

    let grammar_path = language_path.join("tree-sitter");

    std::process::Command::new("git")
      .arg("clone")
      .arg("--depth=1")
      .arg(repo)
      .arg(&grammar_path)
      .status()
      .unwrap();

    std::process::Command::new("cc")
      .args(["-Isrc", "-std=c11", "-fPIC", "-O3", "-c", "-o", "src/parser.o", "src/parser.c"])
      .current_dir(&grammar_path)
      .status()
      .unwrap();
    std::process::Command::new("cc")
      .args(["-Isrc", "-std=c11", "-fPIC", "-O3", "-c", "-o", "src/scanner.o", "src/scanner.c"])
      .current_dir(&grammar_path)
      .status()
      .unwrap();
    std::process::Command::new("cc")
      .args([
        "-O3",
        "-shared",
        "-Wl,-soname,libtree-sitter.so",
        "src/parser.o",
        "src/scanner.o",
        "-o",
        "libtree-sitter.so",
      ])
      .current_dir(&grammar_path)
      .status()
      .unwrap();
  }
}

// See https://github.com/tree-sitter/tree-sitter/wiki/List-of-parsers
fn repo(ft: &FileType) -> Option<&'static str> {
  match ft {
    FileType::Rust => Some("https://github.com/tree-sitter/tree-sitter-rust"),
    FileType::Toml => Some("https://github.com/tree-sitter-grammars/tree-sitter-toml"),
    FileType::Markdown => Some("https://github.com/tree-sitter-grammars/tree-sitter-markdown"),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn it_works() { load_grammar(&FileType::Rust); }
}
