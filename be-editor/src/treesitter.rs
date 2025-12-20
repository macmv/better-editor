use std::{ffi::CString, path::PathBuf};

use tree_sitter::Language;

use crate::filetype::FileType;

#[derive(serde::Deserialize)]
struct TreeSitterSpec {
  grammars: Vec<GrammarSpec>,
}

#[derive(serde::Deserialize)]
struct GrammarSpec {
  name:       String,
  highlights: Vec<String>,
}

pub fn load_grammar(ft: &FileType) {
  if repo(ft).is_none() {
    return;
  }

  let grammar_path = install_grammar(ft).unwrap();

  let spec = std::fs::read_to_string(grammar_path.join("tree-sitter.json")).unwrap();
  let spec = serde_json::from_str::<TreeSitterSpec>(&spec).unwrap();

  if spec.grammars.is_empty() {
    return;
  }

  let so_path = grammar_path.join("libtree-sitter.so");
  let symbol = format!("tree_sitter_{}", spec.grammars[0].name);

  let language = unsafe {
    let so_path_c = CString::new(so_path.to_str().unwrap()).unwrap();
    let object = libc::dlopen(so_path_c.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
    if object.is_null() {
      panic!("Failed to load grammar");
    }
    let symbol = CString::new(symbol).unwrap();
    let language = libc::dlsym(object, symbol.as_ptr());
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

  if !grammar_path.exists() {
    std::process::Command::new("git")
      .arg("clone")
      .arg("--depth=1")
      .arg(repo)
      .arg(&grammar_path)
      .status()
      .unwrap();
  }

  let so_path = grammar_path.join("libtree-sitter.so");
  if !so_path.exists() {
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
