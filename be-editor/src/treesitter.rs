use std::{
  ffi::CString,
  path::{Path, PathBuf},
};

use tree_sitter::Language;

use crate::filetype::FileType;

pub fn load_grammar(ft: &FileType) {
  if repo(ft).is_none() {
    return;
  }

  let grammar_path = install_grammar(ft).unwrap();

  let so_path = grammar_path.join("libtree-sitter.so");

  let language = unsafe {
    let so_path_c = CString::new(so_path.to_str().unwrap()).unwrap();
    let object = libc::dlopen(so_path_c.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
    if object.is_null() {
      panic!("Failed to load grammar");
    }
    let language = libc::dlsym(object, c"tree_sitter_rust".as_ptr());
    if language.is_null() {
      panic!("Failed to load grammar");
    }

    Language::new(std::mem::transmute(language))
  };

  dbg!(&language);
}

fn install_grammar(ft: &FileType) -> Option<PathBuf> {
  let Some(repo) = repo(ft) else { return None };

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

  Some(grammar_path)
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
