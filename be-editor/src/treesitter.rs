use std::{ffi::CString, mem::ManuallyDrop, path::PathBuf};

use be_doc::Document;
use tree_sitter::{
  Language, Node, Parser, Query, QueryCaptures, QueryCursor, StreamingIterator, TextProvider, Tree,
};

use crate::{Change, EditorState, filetype::FileType, highlight::Highlight};

pub struct Highlighter {
  parser:           Parser,
  tree:             Option<Tree>,
  highlights_query: Query,

  // SAFETY: Drop last!
  _language: LoadedLanguage,
}

#[derive(serde::Deserialize)]
struct TreeSitterSpec {
  grammars: Vec<GrammarSpec>,
}

#[derive(serde::Deserialize)]
struct GrammarSpec {
  name:       String,
  highlights: Vec<String>,
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

  let highlights_query = Query::new(
    &language.language,
    &std::fs::read_to_string(grammar_path.join(&grammar.highlights[0])).unwrap(),
  )
  .unwrap();

  Some(Highlighter { parser, tree: None, highlights_query, _language: language })
}

impl EditorState {
  pub(crate) fn on_open_file_highlight(&mut self) {
    let Some(ft) = &self.filetype else { return };

    self.highligher = load_grammar(ft);
  }

  pub(crate) fn offset_to_ts_point(&mut self, offset: usize) -> tree_sitter::Point {
    let row = self.doc.rope.line_of_byte(offset);
    let column = offset - self.doc.rope.byte_of_line(row);

    tree_sitter::Point { row, column }
  }

  pub(crate) fn on_change_highlight(
    &mut self,
    change: &Change,
    start_position: tree_sitter::Point,
    old_end_position: tree_sitter::Point,
  ) {
    let new_end_position = self.offset_to_ts_point(change.range.start + change.text.len());

    let Some(highlighter) = &mut self.highligher else { return };
    if let Some(tree) = &mut highlighter.tree {
      tree.edit(&tree_sitter::InputEdit {
        start_byte: change.range.start,
        old_end_byte: change.range.end,
        new_end_byte: change.range.start + change.text.len(),
        start_position,
        old_end_position,
        new_end_position,
      });
    }

    highlighter.tree =
      Some(highlighter.parser.parse(&change.text, highlighter.tree.as_ref()).unwrap());
  }
}

impl Highlighter {
  fn reparse(&mut self, doc: &Document) {
    self.tree = Some(self.parser.parse(&doc.rope.to_string(), self.tree.as_ref()).unwrap());
  }

  pub(crate) fn highlights<'a>(&'a self, doc: &'a Document) -> Option<CapturesIter<'a>> {
    let Some(tree) = &self.tree else { return None };

    let mut cursor = QueryCursor::new();
    let captures = cursor.captures(&self.highlights_query, tree.root_node(), RopeProvider { doc });
    let captures = unsafe { std::mem::transmute(captures) };

    Some(CapturesIter { query: &self.highlights_query, captures, _cursor: cursor })
  }
}

struct RopeProvider<'a> {
  doc: &'a Document,
}

impl<'a> TextProvider<&'a str> for RopeProvider<'a> {
  type I = be_doc::crop::iter::Chunks<'a>;

  fn text(&mut self, node: Node) -> Self::I {
    let slice = self.doc.rope.byte_slice(node.byte_range());
    slice.chunks()
  }
}

pub(crate) struct CapturesIter<'a> {
  query:    &'a Query,
  captures: QueryCaptures<'a, 'a, RopeProvider<'a>, &'a str>,

  // SAFETY: Drop last, `captures` borrows into this cursor.
  _cursor: QueryCursor,
}

impl<'a> Iterator for CapturesIter<'a> {
  type Item = Highlight<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    let Some((m, index)) = self.captures.next() else { return None };
    let cap = m.captures[*index];

    let start = cap.node.byte_range().start;
    let end = cap.node.byte_range().end;

    let name = self.query.capture_names().get(cap.index as usize).unwrap();

    Some(Highlight { start, end, key: crate::HighlightKey::TreeSitter(name) })
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
  use crate::HighlightKey;

  use super::*;

  #[test]
  fn it_works() {
    let mut highlighter = load_grammar(&FileType::Rust).unwrap();

    let doc = "fn main() {}".into();
    highlighter.reparse(&doc);
    let highlights = highlighter.highlights(&doc).unwrap();

    assert_eq!(
      highlights.collect::<Vec<_>>(),
      [
        Highlight { start: 0, end: 2, key: HighlightKey::TreeSitter("keyword") },
        Highlight { start: 3, end: 7, key: HighlightKey::TreeSitter("function") },
        Highlight { start: 7, end: 8, key: HighlightKey::TreeSitter("punctuation.bracket") },
        Highlight { start: 8, end: 9, key: HighlightKey::TreeSitter("punctuation.bracket") },
        Highlight { start: 10, end: 11, key: HighlightKey::TreeSitter("punctuation.bracket") },
        Highlight { start: 11, end: 12, key: HighlightKey::TreeSitter("punctuation.bracket") },
      ]
    );
  }
}
