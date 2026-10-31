//! The command palette: a searchable list of every action, next to the keys
//! that run it.

use be_input::{Axis, ChangeDirection, Key, KeyStroke, Navigation};
use kurbo::{Point, Rect, Size};

use crate::{Font, render::Render};

/// One action the palette can run.
pub struct Entry {
  pub name: &'static str,
  pub nav:  Navigation,
  /// The keys that run this, if any are bound.
  pub keys: Option<String>,
}

/// What a key or click did to the palette.
pub enum Outcome {
  /// The palette stays open.
  Open,
  Close,
  Run(Navigation),
}

pub struct Palette {
  entries:  Vec<Entry>,
  query:    String,
  /// Indices into `entries` of the ones matching `query`, best match first.
  matches:  Vec<usize>,
  /// A position in `matches`.
  selected: usize,
  /// The first visible position in `matches`.
  scroll:   usize,
}

const WIDTH: f64 = 560.0;
const TOP: f64 = 60.0;
const MAX_ROWS: usize = 10;
const PADDING: f64 = 8.0;
const RADIUS: f64 = 8.0;

/// Every action the palette lists. Actions that act on a workspace are only
/// listed when `in_workspace`.
fn catalog(in_workspace: bool) -> Vec<(&'static str, Navigation, bool)> {
  use Navigation::*;

  vec![
    ("Workspaces: show the list", Workspaces, false),
    ("Workspaces: toggle terminal", OpenWorkspace, false),
    ("File: search files", OpenSearch, true),
    ("File: search symbols", OpenSymbolSearch, true),
    ("Pane: split vertically", Split(Axis::Vertical), true),
    ("Pane: split horizontally", Split(Axis::Horizontal), true),
    ("Pane: close", ClosePane, true),
    ("Pane: toggle zoom", Zoom, true),
    ("Pane: focus next", Cycle(ChangeDirection::Next), true),
    ("Pane: focus previous", Cycle(ChangeDirection::Prev), true),
    ("Pane: focus last", LastPane, true),
    ("Pane: focus up", Direction(be_input::Direction::Up), true),
    ("Pane: focus down", Direction(be_input::Direction::Down), true),
    ("Pane: focus left", Direction(be_input::Direction::Left), true),
    ("Pane: focus right", Direction(be_input::Direction::Right), true),
    ("Tab: new terminal", NewTerminal, true),
    ("Tab: switch to 0", Tab(0), true),
    ("Tab: switch to 1", Tab(1), true),
    ("Tab: switch to 2", Tab(2), true),
    ("Tab: switch to 3", Tab(3), true),
    ("Tab: switch to 4", Tab(4), true),
    ("Tab: switch to 5", Tab(5), true),
    ("Tab: switch to 6", Tab(6), true),
    ("Tab: switch to 7", Tab(7), true),
    ("Tab: switch to 8", Tab(8), true),
    ("Tab: switch to 9", Tab(9), true),
  ]
  .into_iter()
  .filter(|(_, _, workspace)| in_workspace || !workspace)
  .collect()
}

/// The actions the palette lists, with the names it lists them under. This
/// is also the set of actions the which-key popup knows about.
pub fn actions(in_workspace: bool) -> Vec<(&'static str, Navigation)> {
  catalog(in_workspace).into_iter().map(|(name, nav, _)| (name, nav)).collect()
}

/// Fills `Rect::ZERO`-origin `shape` as a floating panel: the background
/// and outline the palette and which-key popup share.
pub fn draw_panel(render: &mut Render, radius: f64) {
  let size = render.size();
  let shape = Rect::from_origin_size(Point::ZERO, size);
  let outline = render.theme().background_raised_outline;

  render.fill(&shape.to_rounded_rect(radius), render.theme().background_raised);
  render.stroke(&shape.inset(-0.5).to_rounded_rect(radius - 0.5), outline, kurbo::Stroke::new(1.0));
}

impl Palette {
  /// `keys` finds the keys bound to an action.
  pub fn new(in_workspace: bool, keys: impl Fn(Navigation) -> Option<String>) -> Self {
    let entries = catalog(in_workspace)
      .into_iter()
      .map(|(name, nav, _)| Entry { name, nav, keys: keys(nav) })
      .collect();

    let mut palette =
      Palette { entries, query: String::new(), matches: vec![], selected: 0, scroll: 0 };
    palette.filter();
    palette
  }

  /// Recomputes `matches` for the current query.
  fn filter(&mut self) {
    let query = self.query.to_lowercase();

    let mut scored = self
      .entries
      .iter()
      .enumerate()
      .filter_map(|(i, e)| score(&e.name.to_lowercase(), &query).map(|s| (s, i)))
      .collect::<Vec<_>>();
    scored.sort();

    self.matches = scored.into_iter().map(|(_, i)| i).collect();
    self.selected = 0;
    self.scroll = 0;
  }

  fn select(&mut self, selected: usize) {
    self.selected = selected;
    if selected < self.scroll {
      self.scroll = selected;
    } else if selected >= self.scroll + MAX_ROWS {
      self.scroll = selected + 1 - MAX_ROWS;
    }
  }

  fn run(&self, position: usize) -> Outcome {
    match self.matches.get(position) {
      Some(&i) => Outcome::Run(self.entries[i].nav),
      None => Outcome::Open,
    }
  }

  pub fn on_key(&mut self, key: &KeyStroke) -> Outcome {
    let count = self.matches.len();

    match key.key {
      Key::Escape => return Outcome::Close,
      Key::Char('\n') => return self.run(self.selected),

      Key::ArrowDown | Key::Tab if !key.shift => {
        if count > 0 {
          self.select((self.selected + 1) % count);
        }
      }
      Key::ArrowUp | Key::Tab => {
        if count > 0 {
          self.select((self.selected + count - 1) % count);
        }
      }
      Key::Char('n') if key.control => {
        if count > 0 {
          self.select((self.selected + 1) % count);
        }
      }
      Key::Char('p') if key.control => {
        if count > 0 {
          self.select((self.selected + count - 1) % count);
        }
      }

      Key::Backspace => {
        self.query.pop();
        self.filter();
      }
      Key::Char(c) if !key.control && !key.alt && !key.sup => {
        self.query.push(c);
        self.filter();
      }

      _ => {}
    }

    Outcome::Open
  }

  /// Where the palette sits in a window of `window`.
  fn bounds(&self, window: Size, line_height: f64) -> Rect {
    let width = WIDTH.min(window.width - 40.0).max(0.0);
    let rows = self.matches.len().min(MAX_ROWS).max(1);
    let height = PADDING * 3.0 + input_height(line_height) + rows as f64 * row_height(line_height);

    Rect::from_origin_size(Point::new((window.width - width) / 2.0, TOP), Size::new(width, height))
  }

  /// A click or hover. Returns the outcome of a click, and moves the
  /// selection to the row under a hover.
  pub fn on_mouse(
    &mut self,
    pos: Point,
    click: bool,
    window: Size,
    line_height: f64,
  ) -> (Outcome, bool) {
    let bounds = self.bounds(window, line_height);

    if !bounds.contains(pos) {
      return (if click { Outcome::Close } else { Outcome::Open }, false);
    }

    let list_top = bounds.y0 + PADDING * 2.0 + input_height(line_height);
    if pos.y < list_top {
      return (Outcome::Open, false);
    }

    let row = self.scroll + ((pos.y - list_top) / row_height(line_height)) as usize;
    if row >= self.matches.len() {
      return (Outcome::Open, false);
    }

    if click {
      (self.run(row), true)
    } else {
      self.selected = row;
      (Outcome::Open, true)
    }
  }

  pub fn draw(&self, render: &mut Render) {
    let window = render.size();
    let line_height = render.store.text.font_metrics(Font::Ui).line_height;
    let bounds = self.bounds(window, line_height);

    render.clipped(bounds, |render| {
      let size = render.size();
      draw_panel(render, RADIUS);

      // The input.
      let input =
        Rect::new(PADDING, PADDING, size.width - PADDING, PADDING + input_height(line_height));
      render.fill(&input.to_rounded_rect(4.0), render.theme().background);

      let origin = Point::new(input.x0 + PADDING, input.y0 + 4.0);
      let typed = render.layout_text(Font::Ui, &self.query, render.theme().text);
      if self.query.is_empty() {
        let hint = render.layout_text(Font::Ui, "Type an action...", render.theme().text_dim);
        render.draw_text(&hint, origin);
      } else {
        render.draw_text(&typed, origin);
      }

      let cursor = typed.cursor(self.query.len(), crate::CursorMode::Line);
      render.fill(&(cursor + origin.to_vec2()), render.theme().text);

      // The rows.
      let list_top = PADDING * 2.0 + input_height(line_height);
      let row = row_height(line_height);

      if self.matches.is_empty() {
        let none = render.layout_text(Font::Ui, "No matching actions", render.theme().text_dim);
        render.draw_text(&none, Point::new(PADDING * 2.0, list_top + 4.0));
      }

      for (i, &entry) in self.matches.iter().enumerate().skip(self.scroll).take(MAX_ROWS) {
        let entry = &self.entries[entry];
        let y = list_top + (i - self.scroll) as f64 * row;

        if i == self.selected {
          let highlight = Rect::new(PADDING, y, size.width - PADDING, y + row);
          render.fill(&highlight.to_rounded_rect(4.0), render.theme().background);
        }

        let name = render.layout_text(Font::Ui, entry.name, render.theme().text);
        render.draw_text(&name, Point::new(PADDING * 2.0, y + 4.0));

        if let Some(keys) = &entry.keys {
          let keys = render.layout_text(Font::Ui, keys, render.theme().text_dim);
          let x = size.width - PADDING * 2.0 - keys.size().width;
          render.draw_text(&keys, Point::new(x, y + 4.0));
        }
      }
    });
  }
}

fn input_height(line_height: f64) -> f64 { line_height + 8.0 }
fn row_height(line_height: f64) -> f64 { line_height + 8.0 }

/// How well `name` matches `query`. Lower is better, and `None` is no match.
/// A match is every character of the query appearing in order.
fn score(name: &str, query: &str) -> Option<(u8, usize)> {
  if query.is_empty() {
    return Some((0, 0));
  }

  if name.starts_with(query) {
    return Some((0, 0));
  }
  if let Some(i) = name.find(query) {
    return Some((1, i));
  }

  let mut chars = name.chars();
  let mut last = 0;
  for q in query.chars().filter(|c| !c.is_whitespace()) {
    last = chars.by_ref().position(|c| c == q)? + last + 1;
  }
  Some((2, last))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn scores_prefer_closer_matches() {
    assert!(score("pane: close", "pane") < score("pane: close", "close"));
    assert!(score("pane: close", "close") < score("pane: close", "pcl"));
    assert_eq!(score("pane: close", "zz"), None);
    assert!(score("pane: close", "").is_some());
  }

  fn palette() -> Palette {
    let config = be_config::Config::default();
    Palette::new(true, |nav| config.settings.keybinds.describe(nav))
  }

  fn key(key: Key) -> KeyStroke {
    KeyStroke { key, sup: false, shift: false, control: false, alt: false }
  }

  #[test]
  fn typing_filters_and_enter_runs() {
    let mut palette = palette();
    let all = palette.matches.len();

    for c in "zoom".chars() {
      palette.on_key(&key(Key::Char(c)));
    }
    assert!(palette.matches.len() < all);

    assert!(matches!(palette.on_key(&key(Key::Char('\n'))), Outcome::Run(Navigation::Zoom)));
  }

  #[test]
  fn escape_closes() {
    assert!(matches!(palette().on_key(&key(Key::Escape)), Outcome::Close));
  }
}
