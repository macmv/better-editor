use std::{ffi::CString, mem::ManuallyDrop, path::PathBuf};

use be_doc::Document;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::filetype::FileType;

pub struct Highlighter {
  parser:           Parser,
  tree:             Tree,
  highlights_query: Query,

  // SAFETY: Drop last!
  language: LoadedLanguage,
}

#[derive(serde::Deserialize)]
struct TreeSitterSpec {
  grammars: Vec<GrammarSpec>,
}

#[derive(serde::Deserialize)]
struct GrammarSpec {
  name:       String,
  highlights: Vec<String>,
  injections: Vec<String>,
}

struct LoadedLanguage {
  object:   *mut libc::c_void,
  language: ManuallyDrop<Language>,
}

pub fn load_grammar(ft: &FileType) -> Option<Highlighter> {
  if repo(ft).is_none() {
    return None;
  }

  let grammar_path = install_grammar(ft).unwrap();

  let spec = std::fs::read_to_string(grammar_path.join("tree-sitter.json")).unwrap();
  let spec = serde_json::from_str::<TreeSitterSpec>(&spec).unwrap();

  if spec.grammars.is_empty() {
    return None;
  }

  let grammar = &spec.grammars[0];

  let so_path = grammar_path.join("libtree-sitter.so");
  let language = LoadedLanguage::load(so_path, &grammar.name);

  let mut parser = Parser::new();
  parser.set_language(&language.language).unwrap();

  let tree = parser.parse("", None).unwrap();

  let highlights_query = Query::new(
    &language.language,
    &std::fs::read_to_string(grammar_path.join(&grammar.highlights[0])).unwrap(),
  )
  .unwrap();

  Some(Highlighter { parser, tree, highlights_query, language })
}

impl Highlighter {
  fn highlights(&self, doc: &Document) {
    let names = self.highlights_query.capture_names();

    let mut cursor = QueryCursor::new();
    let mut captures =
      cursor.captures(&self.highlights_query, self.tree.root_node(), |node: Node| {
        let slice = doc.rope.byte_slice(node.byte_range());
        slice.chunks().map(|c| c.as_bytes())
      });

    while let Some((m, i)) = captures.next() {
      let capture = m.captures[*i];
      println!("node: {}", names[capture.index as usize]);
    }
  }
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

impl LoadedLanguage {
  fn load(so_path: PathBuf, name: &str) -> LoadedLanguage {
    unsafe {
      let so_path_c = CString::new(so_path.to_str().unwrap()).unwrap();
      let object = libc::dlopen(so_path_c.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
      if object.is_null() {
        panic!("Failed to load grammar");
      }
      let symbol = format!("tree_sitter_{}", name);
      let symbol = CString::new(symbol).unwrap();
      let language = libc::dlsym(object, symbol.as_ptr());
      if language.is_null() {
        panic!("Failed to load grammar");
      }

      // `transmute` because I don't want to depend on `tree-sitter-language`, which
      // exports a single transparent wrapper for a language function.
      let language = Language::new(std::mem::transmute(language));

      LoadedLanguage { object, language: ManuallyDrop::new(language) }
    }
  }
}

impl Drop for LoadedLanguage {
  fn drop(&mut self) {
    // SAFETY: Drop the language before closing the object.
    unsafe {
      ManuallyDrop::drop(&mut self.language);
      libc::dlclose(self.object);
    }
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
  fn it_works() {
    let highlighter = load_grammar(&FileType::Rust).unwrap();

    highlighter.highlights(&"fn main() {}".into());
  }
}
