use std::{
  cell::RefCell,
  collections::{HashMap, HashSet},
  ffi::CString,
  mem::ManuallyDrop,
  ops::Range,
  path::{Path, PathBuf},
  rc::Rc,
  sync::{Arc, Mutex},
};

use be_config::{Config, LanguageName};
use be_doc::Document;
use be_macros::ResultExt;
use be_task::Task;
use tree_sitter::{
  Language, Node, Parser, Query, QueryCaptures, QueryCursor, QueryPredicateArg, StreamingIterator,
  TextProvider, Tree,
};

use crate::{Change, EditorState, highlight::Highlight, status::Status};

/// How deeply injections can nest (markdown -> code block -> ...). This only
/// exists to stop a language that injects itself from looping forever.
const MAX_INJECTION_DEPTH: usize = 4;

/// Highlights a document: the grammar for the file's own language, plus a layer
/// for every region of the file that is written in some other language (see
/// `injections.scm` in each grammar).
pub struct Highlighter {
  root:   Layer,
  layers: Vec<Layer>,

  /// Grammars for injected languages that are still being installed.
  pending: Vec<PendingGrammar>,
  /// Injected languages that failed to install, so we don't retry on every
  /// edit.
  failed:  HashSet<LanguageName>,
}

/// A compiled grammar, shared between every document that uses it.
struct Grammar {
  name:       LanguageName,
  highlights: Query,
  injections: Option<Query>,

  // SAFETY: Drop last!
  language: LoadedLanguage,
}

/// One parse tree. The root layer covers the whole document, injected layers
/// only cover the ranges they were injected into.
struct Layer {
  // SAFETY: `parser` and `tree` must drop before `grammar`.
  parser:  Parser,
  tree:    Option<Tree>,
  grammar: Rc<Grammar>,

  /// The byte range from the start of the first included range to the end of
  /// the last. Used to skip layers that are not on screen.
  span: Range<usize>,
}

/// A region of a layer that should be parsed as a different language.
struct Injection {
  language: String,
  ranges:   Vec<tree_sitter::Range>,
}

thread_local! {
  /// Loaded grammars. Grammars are never unloaded, as an injected language is
  /// likely to be used again.
  static GRAMMARS: RefCell<HashMap<LanguageName, Rc<Grammar>>> = RefCell::new(HashMap::new());
}

#[derive(serde::Deserialize)]
struct TreeSitterSpec {
  grammars: Vec<GrammarSpec>,
}

#[derive(serde::Deserialize)]
struct GrammarSpec {
  name:       String,
  highlights: StringOrStrings,
  #[serde(default)]
  injections: Option<StringOrStrings>,
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
  let grammar = load_grammar_cached(ft, grammar_path)?;

  Some(Highlighter {
    root:    Layer::new(grammar, None)?,
    layers:  vec![],
    pending: vec![],
    failed:  HashSet::new(),
  })
}

fn load_grammar_cached(ft: LanguageName, grammar_path: &Path) -> Option<Rc<Grammar>> {
  if let Some(grammar) = GRAMMARS.with(|g| g.borrow().get(&ft).cloned()) {
    return Some(grammar);
  }

  let grammar = Rc::new(load_grammar_uncached(ft, grammar_path)?);
  GRAMMARS.with(|g| g.borrow_mut().insert(ft, grammar.clone()));
  Some(grammar)
}

fn load_grammar_uncached(ft: LanguageName, grammar_path: &Path) -> Option<Grammar> {
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
      injections: Some(StringOrStrings::Vec(vec![
        format!("queries/{}/injections.scm", ft.name()),
        "queries/injections.scm".to_string(),
      ])),
    });
  }

  // Repos like markdown ship several grammars. Pick the one named after this
  // language, falling back to the first.
  let index = spec.grammars.iter().position(|g| g.name == ft.name()).unwrap_or(0);
  let grammar = &spec.grammars[index];

  let so_path = grammar_path.join(so_name);
  let language = LoadedLanguage::load(so_path, &grammar.name);

  let highlights = load_query(&language.language, grammar_path, &grammar.highlights)?;
  let injections =
    grammar.injections.as_ref().and_then(|i| load_query(&language.language, grammar_path, i));

  Some(Grammar { name: ft, highlights, injections, language })
}

/// Loads the first query file in `paths` that exists.
fn load_query(language: &Language, grammar_path: &Path, paths: &StringOrStrings) -> Option<Query> {
  match paths {
    StringOrStrings::String(s) => {
      Query::new(language, &std::fs::read_to_string(grammar_path.join(s)).ok()?).fatal()
    }

    StringOrStrings::Vec(paths) => paths.iter().find_map(|h| {
      // Find the first file that exists.
      let file = std::fs::read_to_string(grammar_path.join(h)).ok()?;

      // Skip any invalid queries, panic in debug mode.
      Query::new(language, &file).fatal()
    }),
  }
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
    if let Some(highlighter) = &mut self.highligher
      && highlighter.poll_pending(&self.doc, &self.config.borrow())
    {
      self.damage_all = true;
    }

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
      highligher.update_injections(&self.doc, &self.config.borrow());
    }
    self.damage_all = true;
  }

  pub(crate) fn offset_to_ts_point(&mut self, offset: usize) -> tree_sitter::Point {
    let len = self.doc.len();
    let offset = offset.min(len);

    // The end of a document that ends in a newline is the start of an
    // (empty) line after the last one, which the line index does not have.
    if len > 0
      && offset == len
      && self.doc.line_with_terminator(be_doc::Line(self.doc.len_lines() - 1)).chars().last()
        == Some('\n')
    {
      return tree_sitter::Point { row: self.doc.len_lines(), column: 0 };
    }

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
    if let Some(tree) = &mut highlighter.root.tree {
      tree.edit(&tree_sitter::InputEdit {
        start_byte: change.range.start,
        old_end_byte: change.range.end,
        new_end_byte: change.range.start + change.text.len(),
        start_position,
        old_end_position,
        new_end_position,
      });
    }

    highlighter.reparse(&self.doc);
    highlighter.update_injections(&self.doc, &self.config.borrow());
  }
}

impl Highlighter {
  /// Reparses the root layer. Injected layers are rebuilt by
  /// `update_injections`.
  fn reparse(&mut self, doc: &Document) { self.root.reparse(doc); }

  /// Finds every region of the document written in another language, and
  /// parses each one.
  ///
  /// This rebuilds all injected layers from scratch. That is cheap for typical
  /// documents, but should become incremental if it shows up in profiles.
  fn update_injections(&mut self, doc: &Document, config: &Config) {
    let mut layers = vec![];
    let mut ctx = InjectionCtx {
      doc,
      config,
      pending: &mut self.pending,
      failed: &mut self.failed,
      out: &mut layers,
    };
    ctx.inject_into(&self.root, 1);

    self.layers = layers;
  }

  /// Applies grammars that have finished installing. Returns true if the
  /// layers changed.
  pub(crate) fn poll_pending(&mut self, doc: &Document, config: &Config) -> bool {
    let mut changed = false;

    let mut i = 0;
    while i < self.pending.len() {
      match self.pending[i].task.completed() {
        Some(res) => {
          let pending = self.pending.swap_remove(i);
          if let Err(e) = res {
            log::error!("failed to install {} grammar: {e}", pending.ft.name());
            self.failed.insert(pending.ft);
          }
          changed = true;
        }
        None => i += 1,
      }
    }

    if changed {
      self.update_injections(doc, config);
    }
    changed
  }

  /// One iterator per layer that overlaps `range`.
  pub(crate) fn highlights<'a>(
    &'a self,
    doc: &'a Document,
    range: Range<usize>,
  ) -> Vec<CapturesIter<'a>> {
    std::iter::once(&self.root)
      .chain(&self.layers)
      .filter(|l| l.span.start <= range.end && l.span.end >= range.start)
      .filter_map(|l| l.highlights(doc, range.clone()))
      .collect()
  }
}

fn parse(parser: &mut Parser, doc: &Document, old: Option<&Tree>) -> Option<Tree> {
  parser.parse_with_options(
    &mut |i, _| {
      if i >= doc.len() { "" } else { doc.range(i..).chunks().next().unwrap_or("") }
    },
    old,
    None,
  )
}

impl Layer {
  /// Makes a layer for `grammar`, and parses it. `ranges` are the parts of the
  /// document that belong to this layer, or `None` for the whole document.
  fn new(grammar: Rc<Grammar>, ranges: Option<&[tree_sitter::Range]>) -> Option<Layer> {
    let mut parser = Parser::new();
    parser.set_language(&grammar.language.language).fatal()?;

    let span = match ranges {
      Some(ranges) => {
        parser.set_included_ranges(ranges).ok()?;
        ranges.first()?.start_byte..ranges.last()?.end_byte
      }
      None => 0..usize::MAX,
    };

    Some(Layer { parser, tree: None, grammar, span })
  }

  fn reparse(&mut self, doc: &Document) {
    self.tree = Some(parse(&mut self.parser, doc, self.tree.as_ref()).unwrap());
  }

  fn highlights<'a>(&'a self, doc: &'a Document, range: Range<usize>) -> Option<CapturesIter<'a>> {
    let tree = self.tree.as_ref()?;

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range);
    let captures =
      cursor.captures(&self.grammar.highlights, tree.root_node(), RopeProvider { doc });
    let captures = unsafe { std::mem::transmute(captures) };

    Some(CapturesIter { query: &self.grammar.highlights, captures, pending: None, _cursor: cursor })
  }

  /// Runs this layer's `injections.scm`, and returns the regions it found.
  fn find_injections(&self, doc: &Document) -> Vec<Injection> {
    let (Some(query), Some(tree)) = (&self.grammar.injections, &self.tree) else { return vec![] };

    let names = query.capture_names();
    let mut out: Vec<Injection> = vec![];
    // Injections marked `injection.combined` share one layer per language.
    let mut combined: HashMap<String, usize> = HashMap::new();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), RopeProvider { doc });

    while let Some(m) = matches.next() {
      let mut language = None;
      let mut is_combined = false;
      let mut include_children = false;

      for prop in query.property_settings(m.pattern_index) {
        match &*prop.key {
          "injection.language" => language = prop.value.as_ref().map(|v| v.to_string()),
          "injection.self" => language = Some(self.grammar.name.name().to_string()),
          "injection.combined" => is_combined = true,
          "injection.include-children" => include_children = true,
          _ => {}
        }
      }

      // `#set-lang-from-info-string! @capture` (typst) names the language with
      // the text of a capture.
      for pred in query.general_predicates(m.pattern_index) {
        if &*pred.operator == "set-lang-from-info-string!"
          && let Some(QueryPredicateArg::Capture(id)) = pred.args.first()
          && let Some(cap) = m.captures.iter().find(|c| c.index == *id)
        {
          language = Some(node_text(doc, cap.node));
        }
      }

      let mut ranges = vec![];
      for cap in m.captures {
        match names[cap.index as usize] {
          "injection.language" => language = Some(node_text(doc, cap.node)),
          "injection.content" => ranges.extend(content_ranges(cap.node, include_children)),
          _ => {}
        }
      }

      // Without a language there is nothing to parse. This is also how
      // `@injection.shebang` matches are skipped.
      let Some(language) = language else { continue };
      if ranges.is_empty() {
        continue;
      }

      if is_combined && let Some(&i) = combined.get(&language) {
        out[i].ranges.extend(ranges);
      } else {
        if is_combined {
          combined.insert(language.clone(), out.len());
        }
        out.push(Injection { language, ranges });
      }
    }

    for injection in &mut out {
      injection.ranges.sort_by_key(|r| r.start_byte);
    }

    out
  }
}

struct InjectionCtx<'a> {
  doc:     &'a Document,
  config:  &'a Config,
  pending: &'a mut Vec<PendingGrammar>,
  failed:  &'a mut HashSet<LanguageName>,
  out:     &'a mut Vec<Layer>,
}

impl InjectionCtx<'_> {
  fn inject_into(&mut self, parent: &Layer, depth: usize) {
    let injections = parent.find_injections(self.doc);
    self.inject(injections, depth);
  }

  fn inject(&mut self, injections: Vec<Injection>, depth: usize) {
    if depth > MAX_INJECTION_DEPTH {
      return;
    }

    for injection in injections {
      let Some(ft) = resolve_language(self.config, &injection.language) else { continue };
      let Some(grammar) = self.grammar_for(ft) else { continue };
      let Some(mut layer) = Layer::new(grammar, Some(&injection.ranges)) else { continue };

      layer.reparse(self.doc);
      let children = layer.find_injections(self.doc);

      // Children go after their parent, so deeper layers draw on top.
      self.out.push(layer);
      self.inject(children, depth + 1);
    }
  }

  /// The loaded grammar for `ft`. If it is not installed yet, this starts
  /// installing it, and returns `None` until that finishes.
  fn grammar_for(&mut self, ft: LanguageName) -> Option<Rc<Grammar>> {
    if self.failed.contains(&ft) {
      return None;
    }

    let job = GrammarJob::new(self.config, ft)?;
    match job.installed_path() {
      Some(path) => load_grammar_cached(ft, &path),
      None => {
        if !self.pending.iter().any(|p| p.ft == ft) {
          self.pending.push(job.spawn());
        }
        None
      }
    }
  }
}

/// Finds the configured language an injection refers to. This accepts the
/// language's name (`rust`), its display name, or a file extension (`rs`).
fn resolve_language(config: &Config, name: &str) -> Option<LanguageName> {
  let name = name.trim().to_lowercase();

  config
    .languages
    .iter()
    .find(|(k, l)| {
      k.name() == name
        || l.display_name.to_lowercase() == name
        || l.extensions.iter().any(|e| *e == name)
    })
    .map(|(k, _)| *k)
}

fn node_text(doc: &Document, node: Node) -> String {
  doc.range(node.byte_range()).chunks().collect()
}

/// The ranges of a content node that belong to the injected language. Unless
/// `include_children` is set, the ranges of named child nodes are cut out.
///
/// Anonymous children are kept: markdown's block grammar puts the backticks of
/// a code span in an `inline` node as anonymous tokens, and cutting those out
/// would split the inline content into pieces that can't be parsed.
fn content_ranges(node: Node, include_children: bool) -> Vec<tree_sitter::Range> {
  let range = |start_byte, end_byte, start_point, end_point| tree_sitter::Range {
    start_byte,
    end_byte,
    start_point,
    end_point,
  };

  if include_children {
    return vec![node.range()];
  }

  let mut ranges = vec![];
  let mut byte = node.start_byte();
  let mut point = node.start_position();

  let mut walk = node.walk();
  for child in node.children(&mut walk).filter(|c| c.is_named()) {
    if child.start_byte() > byte {
      ranges.push(range(byte, child.start_byte(), point, child.start_position()));
    }
    byte = child.end_byte();
    point = child.end_position();
  }

  if node.end_byte() > byte {
    ranges.push(range(byte, node.end_byte(), point, node.end_position()));
  }

  ranges
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
    let highlights = highlighter.highlights(&doc, 0..doc.len());

    assert_eq!(
      highlights.into_iter().flatten().collect::<Vec<_>>(),
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
    let highlights = highlighter.highlights(&doc, 0..doc.len());

    assert!(highlights.into_iter().flatten().count() > 0);
  }

  /// Highlights for `doc`, with injected layers, for the language `lang`.
  fn highlight_with_injections(
    lang: &str,
    text: &str,
    preinstall: &[&str],
  ) -> Vec<(usize, usize, String)> {
    let config = Config::default_ref();
    for name in preinstall {
      let ft = *config.languages.keys().find(|k| k.name() == *name).unwrap();
      GrammarJob::new(config, ft).unwrap().install(&Mutex::new("")).unwrap();
    }

    let ft = *config.languages.keys().find(|k| k.name() == lang).unwrap();
    let mut highlighter = load_grammar(config, ft).unwrap();

    let doc: Document = text.into();
    highlighter.reparse(&doc);
    highlighter.update_injections(&doc, config);

    highlighter
      .highlights(&doc, 0..doc.len())
      .into_iter()
      .flatten()
      .map(|h| match h.key {
        crate::HighlightKey::TreeSitter(k) => (h.start, h.end, k.to_string()),
        _ => unreachable!(),
      })
      .collect()
  }

  #[test]
  fn typst_raw_block_injection() {
    let text = "```rust\nfn main() {}\n```\n";
    let hl = highlight_with_injections("typst", text, &["rust"]);

    // `fn` is a keyword, but only if the block was parsed as rust.
    let fn_start = text.find("fn").unwrap();
    assert!(
      hl.contains(&(fn_start, fn_start + 2, "keyword".to_string())),
      "no rust keyword in {hl:?}"
    );
  }

  #[test]
  fn markdown_fenced_code_injection() {
    let text = "# Title\n\n```rust\nfn main() {}\n```\n";
    let hl = highlight_with_injections("markdown", text, &["rust", "markdown_inline"]);

    let fn_start = text.find("fn").unwrap();
    assert!(
      hl.contains(&(fn_start, fn_start + 2, "keyword".to_string())),
      "no rust keyword in {hl:?}"
    );
  }

  #[test]
  fn markdown_inline_injection() {
    let text = "some `code` here\n";
    let hl = highlight_with_injections("markdown", text, &["markdown_inline"]);

    // The code span only exists in the inline grammar.
    let start = text.find("`code`").unwrap();
    assert!(
      hl.iter().any(|(s, e, _)| *s >= start && *e <= start + 6),
      "no inline highlight in {hl:?}"
    );
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
