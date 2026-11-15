use std::{
  ffi::CString,
  mem::ManuallyDrop,
  ops::Range,
  path::{Path, PathBuf},
  sync::{Arc, Mutex},
};

use be_config::{Config, LanguageName};
use be_doc::Document;
use be_macros::ResultExt;
use be_task::Task;
use tree_sitter::{
  Language, Node, Parser, Query, QueryCaptures, QueryCursor, StreamingIterator, TextProvider, Tree,
};

use crate::{Change, EditorState, highlight::Highlight, status::Status};

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
  highlights: StringOrStrings,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StringOrStrings {
  String(String),
  Vec(Vec<String>),
}

struct LoadedLanguage {
  object:   *mut libc::c_void,
  language: ManuallyDrop<Language>,
}

/// Loads the grammar for `ft`, installing it first if needed. This blocks while
/// the grammar is cloned and compiled, so the editor uses `GrammarJob::spawn`
/// instead.
#[cfg(test)]
fn load_grammar(config: &Config, ft: LanguageName) -> Option<Highlighter> {
  let job = GrammarJob::new(config, ft)?;
  let path = job.install(&Mutex::new("")).fatal()?;
  load_installed(ft, &path)
}

/// Loads a grammar that has already been installed at `grammar_path`.
fn load_installed(ft: LanguageName, grammar_path: &Path) -> Option<Highlighter> {
  #[cfg(target_os = "linux")]
  let so_name = "libtree-sitter.so";
  #[cfg(target_os = "macos")]
  let so_name = "libtree-sitter.dylib";

  let spec = std::fs::read_to_string(grammar_path.join("tree-sitter.json")).fatal()?;
  let mut spec = serde_json::from_str::<TreeSitterSpec>(&spec).fatal()?;

  // Some grammars (such as typst) ship a `tree-sitter.json` with no grammars
  // listed. Fall back to the conventional layout: a `tree_sitter_<lang>`
  // symbol, and queries in `queries/<lang>/highlights.scm`.
  if spec.grammars.is_empty() {
    spec.grammars.push(GrammarSpec {
      name:       ft.name().to_string(),
      highlights: StringOrStrings::Vec(vec![
        format!("queries/{}/highlights.scm", ft.name()),
        "queries/highlights.scm".to_string(),
      ]),
    });
  }

  let grammar = &spec.grammars[0];

  let so_path = grammar_path.join(so_name);
  let language = LoadedLanguage::load(so_path, &grammar.name);

  let mut parser = Parser::new();
  parser.set_language(&language.language).fatal()?;

  let highlights_query = match &grammar.highlights {
    StringOrStrings::String(s) => {
      Query::new(&language.language, &std::fs::read_to_string(grammar_path.join(s)).fatal()?)
        .fatal()?
    }

    StringOrStrings::Vec(highlights) => highlights.iter().find_map(|h| {
      // Find the first file that exists.
      let file = std::fs::read_to_string(grammar_path.join(h)).ok()?;

      // Skip any invalid queries, panic in debug mode.
      Query::new(&language.language, &file).fatal()
    })?,
  };

  Some(Highlighter { parser, tree: None, highlights_query, _language: language })
}

/// A grammar that is being cloned and compiled on another thread.
pub(crate) struct PendingGrammar {
  ft:    LanguageName,
  task:  Task<Result<PathBuf, String>>,
  /// What the thread is doing right now, for the progress display.
  stage: Arc<Mutex<&'static str>>,
}

impl PendingGrammar {
  pub fn status(&self) -> String {
    format!("{} grammar: {}", self.ft.name(), self.stage.lock().unwrap_or_else(|e| e.into_inner()))
  }
}

impl EditorState {
  pub(crate) fn on_open_file_highlight(&mut self) {
    // Dropping the old pending grammar throws its result away. The thread
    // still finishes, so the next open finds the grammar installed.
    self.pending_grammar = None;
    self.highligher = None;

    let Some(ft) = self.filetype else { return };
    let Some(job) = GrammarJob::new(&self.config.borrow(), ft) else { return };

    match job.installed_path() {
      Some(path) => self.finish_grammar(ft, &path),
      None => self.pending_grammar = Some(job.spawn()),
    }
  }

  /// Applies a grammar that has finished installing in the background.
  pub(crate) fn update_grammar(&mut self) {
    let Some(pending) = &self.pending_grammar else { return };
    let Some(res) = pending.task.completed() else { return };
    let ft = pending.ft;
    self.pending_grammar = None;

    match res {
      Ok(path) => self.finish_grammar(ft, &path),
      Err(e) => {
        log::error!("failed to install {} grammar: {e}", ft.name());
        self.status = Some(Status {
          message: format!("failed to install {} grammar: {e}", ft.name()),
          success: false,
        });
      }
    }
  }

  fn finish_grammar(&mut self, ft: LanguageName, path: &Path) {
    self.highligher = load_installed(ft, path);
    if let Some(highligher) = &mut self.highligher {
      highligher.reparse(&self.doc);
    }
    self.damage_all = true;
  }

  pub(crate) fn offset_to_ts_point(&mut self, offset: usize) -> tree_sitter::Point {
    let row = self.doc.line_of_byte(offset);
    let column = offset - self.doc.byte_of_line(row);

    tree_sitter::Point { row: row.as_usize(), column }
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

    highlighter.tree = Some(
      highlighter
        .parser
        .parse_with_options(
          &mut |i, _| {
            if i >= self.doc.len() { "" } else { self.doc.range(i..).chunks().next().unwrap_or("") }
          },
          highlighter.tree.as_ref(),
          None,
        )
        .unwrap(),
    );
  }
}

impl Highlighter {
  fn reparse(&mut self, doc: &Document) {
    self.tree = Some(
      self
        .parser
        .parse_with_options(
          &mut |i, _| {
            if i >= doc.len() { "" } else { doc.range(i..).chunks().next().unwrap_or("") }
          },
          self.tree.as_ref(),
          None,
        )
        .unwrap(),
    );
  }

  pub(crate) fn highlights<'a>(
    &'a self,
    doc: &'a Document,
    range: Range<usize>,
  ) -> Option<CapturesIter<'a>> {
    let Some(tree) = &self.tree else { return None };

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range);
    let captures = cursor.captures(&self.highlights_query, tree.root_node(), RopeProvider { doc });
    let captures = unsafe { std::mem::transmute(captures) };

    Some(CapturesIter { query: &self.highlights_query, captures, pending: None, _cursor: cursor })
  }
}

struct RopeProvider<'a> {
  doc: &'a Document,
}

impl<'a> TextProvider<&'a str> for RopeProvider<'a> {
  type I = be_rope::iter::Chunks<'a>;

  fn text(&mut self, node: Node) -> Self::I {
    let slice = self.doc.range(node.byte_range());
    slice.chunks()
  }
}

pub(crate) struct CapturesIter<'a> {
  query:    &'a Query,
  captures: QueryCaptures<'a, 'a, RopeProvider<'a>, &'a str>,

  // The last capture seen, and the index of the pattern that produced it. Held
  // back until we know no later pattern captures the same node.
  pending: Option<(Highlight<'a>, usize)>,

  // SAFETY: Drop last, `captures` borrows into this cursor.
  _cursor: QueryCursor,
}

impl<'a> CapturesIter<'a> {
  fn next_raw(&mut self) -> Option<(Highlight<'a>, usize)> {
    let (m, index) = self.captures.next()?;
    let cap = m.captures[*index];
    let pattern = m.pattern_index;

    let range = cap.node.byte_range();
    let name = self.query.capture_names().get(cap.index as usize).unwrap();

    Some((
      Highlight {
        start: range.start,
        end:   range.end,
        key:   crate::HighlightKey::TreeSitter(name),
      },
      pattern,
    ))
  }
}

impl<'a> Iterator for CapturesIter<'a> {
  type Item = Highlight<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    // When multiple patterns capture the exact same node, only the one defined
    // last in the query applies. This matches tree-sitter's own highlighter,
    // and is how queries make specific patterns (`@type`) override generic
    // ones (`@constructor` on every capitalized identifier).
    loop {
      let Some((hl, pattern)) = self.next_raw() else {
        return self.pending.take().map(|(h, _)| h);
      };

      match self.pending.take() {
        None => self.pending = Some((hl, pattern)),
        Some((prev, prev_pattern)) => {
          if prev.start == hl.start && prev.end == hl.end {
            self.pending =
              Some(if pattern >= prev_pattern { (hl, pattern) } else { (prev, prev_pattern) });
          } else {
            self.pending = Some((hl, pattern));
            return Some(prev);
          }
        }
      }
    }
  }
}

/// Only one grammar is installed at a time, so two editors opening the same
/// language don't clone into the same directory at once.
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

/// Everything needed to install a grammar. Owned, so it can move to a thread.
struct GrammarJob {
  ft:         LanguageName,
  repo:       String,
  inner_path: PathBuf,
}

impl GrammarJob {
  fn new(config: &Config, ft: LanguageName) -> Option<GrammarJob> {
    let settings = config.languages.get(&ft)?.tree_sitter.as_ref()?;

    Some(GrammarJob {
      ft,
      repo: settings.repo.clone(),
      inner_path: PathBuf::from(settings.path.as_deref().unwrap_or(".")),
    })
  }

  fn grammar_path(&self) -> PathBuf {
    std::env::home_dir()
      .unwrap()
      .join(".local")
      .join("share")
      .join("be")
      .join("language")
      .join(self.ft.name())
      .join("tree-sitter")
  }

  /// The grammar's directory, if it is already built.
  fn installed_path(&self) -> Option<PathBuf> {
    let path = self.grammar_path();
    path.join(SO_NAME).exists().then_some(path)
  }

  fn spawn(self) -> PendingGrammar {
    let ft = self.ft;
    let task = Task::new();
    let completer = task.completer();
    let stage = Arc::new(Mutex::new("waiting"));

    let thread_stage = stage.clone();
    std::thread::spawn(move || {
      let res = self.install(&thread_stage);
      let _ = completer.complete(res);
    });

    PendingGrammar { ft, task, stage }
  }

  /// Clones and compiles the grammar, if that hasn't been done. Safe to call
  /// again once it has succeeded.
  fn install(&self, stage: &Mutex<&'static str>) -> Result<PathBuf, String> {
    let set_stage = |s| *stage.lock().unwrap_or_else(|e| e.into_inner()) = s;

    let _guard = INSTALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let grammar_path = self.grammar_path();
    std::fs::create_dir_all(grammar_path.parent().unwrap()).map_err(|e| e.to_string())?;

    if !grammar_path.exists() {
      set_stage("cloning");
      let res = run(
        std::process::Command::new("git")
          .args(["clone", "--depth=1", &self.repo])
          .arg(&grammar_path),
      );
      if let Err(e) = res {
        // Don't leave a half-cloned directory that looks installed.
        let _ = std::fs::remove_dir_all(&grammar_path);
        return Err(e);
      }
    }

    if grammar_path.join(SO_NAME).exists() {
      return Ok(grammar_path);
    }

    set_stage("compiling");
    let source_path = grammar_path.join(&self.inner_path).join("src");
    let include = format!("-I{}", source_path.display());

    let compile = |compiler: &str, std: &str, name: &str| {
      let mut cmd = std::process::Command::new(compiler);
      cmd
        .args([&include as &str, std, "-fPIC", "-O3", "-c", "-o"])
        .arg(source_path.join("scanner.o"))
        .arg(source_path.join(name))
        .current_dir(&grammar_path);
      run(&mut cmd)
    };

    run(
      std::process::Command::new("cc")
        .args([&include as &str, "-std=c11", "-fPIC", "-O3", "-c", "-o"])
        .arg(source_path.join("parser.o"))
        .arg(source_path.join("parser.c"))
        .current_dir(&grammar_path),
    )?;

    let mut obj_files = vec![source_path.join("parser.o")];
    let mut has_cpp = false;

    if source_path.join("scanner.c").exists() {
      compile("cc", "-std=c11", "scanner.c")?;
      obj_files.push(source_path.join("scanner.o"));
    } else if source_path.join("scanner.cc").exists() {
      has_cpp = true;
      compile("c++", "-std=c++14", "scanner.cc")?;
      obj_files.push(source_path.join("scanner.o"));
    }

    set_stage("linking");

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    compile_error!("tree sitter not setup for target os");

    let linker = if has_cpp { "c++" } else { "cc" };
    let mut cmd = std::process::Command::new(linker);

    #[cfg(target_os = "linux")]
    cmd.args(["-O3", "-shared", "-Wl,-soname,libtree-sitter.so"]);

    #[cfg(target_os = "macos")]
    cmd.args(["-O3", "-dynamiclib", "-Wl,-install_name,libtree-sitter.dylib"]);

    for obj in &obj_files {
      cmd.arg(obj);
    }
    cmd.arg("-o").arg(SO_NAME);

    run(cmd.current_dir(&grammar_path))?;

    Ok(grammar_path)
  }
}

#[cfg(target_os = "linux")]
const SO_NAME: &str = "libtree-sitter.so";
#[cfg(target_os = "macos")]
const SO_NAME: &str = "libtree-sitter.dylib";

/// Runs `cmd` to completion, with its output captured for the error message.
fn run(cmd: &mut std::process::Command) -> Result<(), String> {
  let output = cmd.output().map_err(|e| format!("failed to run {:?}: {e}", cmd.get_program()))?;

  if output.status.success() {
    Ok(())
  } else {
    Err(format!(
      "{:?} failed with {}: {}",
      cmd.get_program(),
      output.status,
      String::from_utf8_lossy(&output.stderr).trim()
    ))
  }
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

      // `transmute` because I don't want to depend on `tree-sitter-language`,
      // which exports a single transparent wrapper for a language
      // function.
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

#[cfg(test)]
mod tests {
  use crate::HighlightKey;

  use super::*;

  #[test]
  fn it_works() {
    let config = Config::default_ref();
    let name = config.languages.keys().find(|k| k.name() == "rust").unwrap();
    let mut highlighter = load_grammar(config, *name).unwrap();

    let doc = "fn main() {}".into();
    highlighter.reparse(&doc);
    let highlights = highlighter.highlights(&doc, 0..doc.len()).unwrap();

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

  #[test]
  fn typst_without_grammar_list() {
    let config = Config::default_ref();
    let name = config.languages.keys().find(|k| k.name() == "typst").unwrap();
    let mut highlighter = load_grammar(config, *name).unwrap();

    let doc = "= Title\n".into();
    highlighter.reparse(&doc);
    let highlights = highlighter.highlights(&doc, 0..doc.len()).unwrap();

    assert!(highlights.count() > 0);
  }

  #[test]
  fn background_install() {
    let config = Config::default_ref();
    let name = config.languages.keys().find(|k| k.name() == "rust").unwrap();
    let job = GrammarJob::new(config, *name).unwrap();

    let pending = job.spawn();
    let path = loop {
      if let Some(res) = pending.task.completed() {
        break res.unwrap();
      }
      std::thread::sleep(std::time::Duration::from_millis(5));
    };

    assert!(path.join(SO_NAME).exists());
    assert!(load_installed(*name, &path).is_some());
  }
}
