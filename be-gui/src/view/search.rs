use std::path::PathBuf;

use be_input::{Action, Direction, Edit, Move};
use kurbo::{Point, Rect, RoundedRect, Stroke};
use unicode_segmentation::UnicodeSegmentation;

use crate::{Notify, Render};

pub struct Search {
  index:   Index,
  results: Vec<String>,
  notify:  Notify,

  search: String,
  cursor: usize, // in bytes
}

// TODO:
// - Source from LSP document symbols or other things.
// - Use an actualy ngram index.
// - Use fuzzy find.
// - Make this lazily populate on another thread.
// - Prioritize non-gitignore'd files.
struct Index {
  entries: Vec<String>,
}

impl Search {
  pub fn new(notify: Notify) -> Self {
    let mut search =
      Search { index: Index::new(), results: vec![], search: String::new(), cursor: 0, notify };
    search.update();
    search
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

    let result_count_fract =
      (render.size().height - 60.0) / render.store.text.font_metrics().line_height;
    let result_count = result_count_fract.floor() as usize;

    for (i, result) in self.results.iter().rev().take(result_count).enumerate() {
      let y = render.size().height - 60.0 - i as f64 * render.store.text.font_metrics().line_height;
      let layout = render.layout_text(result, render.theme().text);
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

    let layout = render.layout_text(&self.search, render.theme().text);
    let text_pos = Point::new(20.0, render.size().height - 40.0);
    render.draw_text(&layout, text_pos);

    let cursor = layout.cursor(self.cursor as usize, crate::CursorMode::Line);
    render.fill(&(cursor + text_pos.to_vec2()), render.theme().text);
  }

  fn update(&mut self) {
    self.results.clear();
    self.results.extend(self.index.entries.iter().filter(|f| f.contains(&self.search)).cloned());
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { m: Move::Single(Direction::Left), .. } => self.move_cursor(-1),
      Action::Move { m: Move::Single(Direction::Right), .. } => self.move_cursor(1),

      Action::Edit { e: Edit::Insert('\n'), .. } => {
        if let Some(result) = self.results.last() {
          self.notify.open_file(PathBuf::from(result));
        }
      }
      Action::Edit { e: Edit::Insert(c), .. } => {
        self.search.insert(self.cursor, c);
        self.update();
        self.move_cursor(1);
      }
      Action::Edit { e: Edit::Delete(Move::Single(Direction::Right)), .. } => {
        self.delete_graphemes(1);
        self.update();
      }
      Action::Edit { e: Edit::Backspace, .. } => {
        if self.cursor > 0 {
          self.move_cursor(-1);
          self.delete_graphemes(1);
          self.update();
        }
      }

      _ => {}
    }
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

impl Index {
  pub fn new() -> Self {
    let mut files = vec![];

    let _start = std::time::Instant::now();
    recurse(".", &mut files);
    dbg!(_start.elapsed());

    Index { entries: files }
  }
}

fn recurse(path: &str, files: &mut Vec<String>) {
  for entry in std::fs::read_dir(path).unwrap() {
    let entry = entry.unwrap();
    let path = entry.path();

    // TODO: Optimize `Index` so we don't need this.
    if path.file_name().is_some_and(|name| name == "target" || name == ".git") {
      continue;
    }

    if path.is_dir() {
      recurse(path.to_str().unwrap(), files);
    } else {
      files.push(path.to_str().unwrap().to_string());
    }
  }
}
