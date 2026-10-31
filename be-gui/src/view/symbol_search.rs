use std::{cell::RefCell, rc::Rc};

use be_editor::{CursorOrLsp, EditorEvent};
use be_fs::WorkspaceRoot;
use be_input::{Action, Direction, Edit, Move};
use be_lsp::{
  LanguageServerStore,
  command::{WorkspaceSymbol, WorkspaceSymbols},
  types,
};
use be_task::Task;
use kurbo::{Point, Rect, RoundedRect, Stroke};
use unicode_segmentation::UnicodeSegmentation;

use crate::{Notify, Render};

/// Searches the workspace's symbols through `workspace/symbol`.
///
/// The language server does the filtering, so every change to the query sends
/// a new request. Responses to older queries are dropped by dropping their
/// tasks.
pub struct SymbolSearch {
  root:   WorkspaceRoot,
  store:  Rc<RefCell<LanguageServerStore>>,
  notify: Notify,

  search: String,
  cursor: usize, // in bytes

  /// Requests for the current query that haven't been answered.
  tasks:    Vec<Task<Vec<WorkspaceSymbol>>>,
  /// Answers to the current query so far.
  incoming: Vec<WorkspaceSymbol>,
  /// The last answer shown. Kept until the next query is answered, so the list
  /// doesn't flicker while typing.
  results:  Vec<WorkspaceSymbol>,
  /// Set if any language server could take the last request.
  capable:  bool,
  /// Counted from the bottom of the list, where the best match is drawn.
  selected: usize,
}

impl SymbolSearch {
  pub fn new(root: WorkspaceRoot, store: Rc<RefCell<LanguageServerStore>>, notify: Notify) -> Self {
    let mut search = SymbolSearch {
      root,
      store,
      notify,
      search: String::new(),
      cursor: 0,
      tasks: vec![],
      incoming: vec![],
      results: vec![],
      capable: false,
      selected: 0,
    };
    search.send_query();
    search
  }

  pub fn layout(&mut self) {
    let mut updated = false;

    let incoming = &mut self.incoming;
    self.tasks.retain(|task| match task.completed() {
      Some(symbols) => {
        incoming.extend(symbols);
        updated = true;
        false
      }
      None => true,
    });

    if updated {
      self.results = self.incoming.clone();
      self.selected = 0;
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    let bounds = Rect::from_origin_size(Point::ZERO, render.size());

    let radius = 20.0;
    render.fill(&RoundedRect::from_rect(bounds, radius), render.theme().background_raised);
    let stroke = 1.0 / render.scale();
    render.stroke(
      &RoundedRect::from_rect(bounds.inset(-stroke), radius),
      render.theme().background_raised_outline,
      Stroke::new(stroke),
    );

    let line_height = render.store.text.font_metrics(crate::Font::Editor).line_height;
    let result_count = ((render.size().height - 60.0) / line_height).floor() as usize;

    if !self.capable {
      let layout = render.layout_text(
        crate::Font::Editor,
        "no language server supports workspace symbols",
        render.theme().text_dim,
      );
      render.draw_text(&layout, Point::new(20.0, render.size().height - 60.0 - line_height));
    }

    for (i, symbol) in self.results.iter().take(result_count).enumerate() {
      let y = render.size().height - 60.0 - i as f64 * line_height;

      if i == self.selected {
        render.fill(
          &Rect::new(10.0, y, render.size().width - 10.0, y + line_height),
          render.theme().background,
        );
      }

      let detail = self.detail(symbol);
      let text = format!("{}  {}", symbol.name, detail);

      let dim = render.theme().text_dim;
      let mut builder = render.store.text.layout_builder(
        crate::Font::Editor,
        &text,
        render.theme().text,
        render.scale(),
      );
      builder.apply(symbol.name.len()..text.len(), parley::StyleProperty::Brush(dim.into()));

      let (built, backgrounds) = builder.build(&text);
      let layout = render.build_layout(crate::Font::Editor, built, backgrounds);
      render.draw_text(&layout, Point::new(20.0, y));
    }

    let bounds = Rect::new(
      20.0,
      render.size().height - 40.0,
      render.size().width - 20.0,
      render.size().height - 20.0,
    );
    render.fill(&bounds, render.theme().background);
    render.stroke(&bounds, render.theme().background_raised_outline, Stroke::new(stroke));

    let layout = render.layout_text(crate::Font::Editor, &self.search, render.theme().text);
    let text_pos = Point::new(20.0, render.size().height - 40.0);
    render.draw_text(&layout, text_pos);

    let cursor = layout.cursor(self.cursor as usize, crate::CursorMode::Line);
    render.fill(&(cursor + text_pos.to_vec2()), render.theme().text);
  }
}

impl SymbolSearch {
  /// The kind, container, and file of a symbol, for display next to its name.
  fn detail(&self, symbol: &WorkspaceSymbol) -> String {
    let mut detail = kind_label(symbol.kind).to_string();

    if let Some(container) = &symbol.container
      && !container.is_empty()
    {
      detail.push_str(&format!("  {container}"));
    }

    if let Some(path) = symbol.uri.to_file_path() {
      let path = path.strip_prefix(self.root.as_path()).unwrap_or(&path);
      detail.push_str(&format!("  {}", path.display()));
    }

    detail
  }

  /// Replaces any outstanding requests with one for the current query.
  fn send_query(&mut self) {
    let command = WorkspaceSymbols { query: self.search.clone() };

    self.tasks = self.store.borrow().send(&command);
    self.capable = !self.tasks.is_empty();
    self.incoming.clear();

    if !self.capable {
      self.results.clear();
    }
  }

  fn open_selected(&self) {
    let Some(symbol) = self.results.get(self.selected) else { return };
    let Some(path) = symbol.uri.to_file_path() else { return };

    self.notify.editor_event(EditorEvent::OpenFile(
      path,
      Some(CursorOrLsp::Lsp { encoding: symbol.encoding, pos: symbol.pos.clone() }),
    ));
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { m: Move::Single(Direction::Left), .. } => self.move_cursor(-1),
      Action::Move { m: Move::Single(Direction::Right), .. } => self.move_cursor(1),
      // The best match is at the bottom, so up is further down the results.
      Action::Move { m: Move::Single(Direction::Up), .. }
      | Action::MoveCompletion { next: false } => {
        if self.selected + 1 < self.results.len() {
          self.selected += 1;
        }
      }
      Action::Move { m: Move::Single(Direction::Down), .. }
      | Action::MoveCompletion { next: true } => {
        self.selected = self.selected.saturating_sub(1);
      }

      Action::Edit { e: Edit::Insert('\n'), .. } => self.open_selected(),
      Action::Edit { e: Edit::Insert(c), .. } => {
        self.search.insert(self.cursor, c);
        self.move_cursor(1);
        self.send_query();
      }
      Action::Edit { e: Edit::Delete(Move::Single(Direction::Right)), .. } => {
        self.delete_graphemes(1);
        self.send_query();
      }
      Action::Edit { e: Edit::Backspace, .. } => {
        if self.cursor > 0 {
          self.move_cursor(-1);
          self.delete_graphemes(1);
          self.send_query();
        }
      }

      _ => {}
    }

    self.notify.wake();
  }

  fn move_cursor(&mut self, dist: i32) {
    if dist >= 0 {
      for c in self.search[self.cursor..].graphemes(true).take(dist as usize) {
        self.cursor += c.len();
      }
    } else {
      for c in self.search[..self.cursor].graphemes(true).rev().take(-dist as usize) {
        self.cursor -= c.len();
      }
    }
  }

  fn delete_graphemes(&mut self, len: usize) {
    let count =
      self.search[self.cursor..].graphemes(true).take(len).map(|g| g.len()).sum::<usize>();
    self.search.replace_range(self.cursor..self.cursor + count, "");
  }
}

fn kind_label(kind: types::SymbolKind) -> &'static str {
  use types::SymbolKind::*;

  match kind {
    File => "file",
    Module => "module",
    Namespace => "namespace",
    Package => "package",
    Class => "class",
    Method => "method",
    Property => "property",
    Field => "field",
    Constructor => "constructor",
    Enum => "enum",
    Interface => "interface",
    Function => "fn",
    Variable => "variable",
    Constant => "const",
    String => "string",
    Number => "number",
    Boolean => "bool",
    Array => "array",
    Object => "object",
    Key => "key",
    Null => "null",
    EnumMember => "variant",
    Struct => "struct",
    Event => "event",
    Operator => "operator",
    TypeParameter => "type param",
  }
}
