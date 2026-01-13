use std::path::PathBuf;

use be_input::{Action, Direction, Edit, Move};
use kurbo::{Point, Rect, RoundedRect, Stroke};
use nucleo::{Injector, Nucleo};
use unicode_segmentation::UnicodeSegmentation;

use crate::{Notify, Render};

pub struct Search {
  index:  Index,
  notify: Notify,

  search: String,
  cursor: usize, // in bytes
}

// TODO:
// - Source from LSP document symbols or other things.
// - Prioritize non-gitignore'd files.
struct Index {
  nucleo:  Nucleo<String>,
  matcher: nucleo::Matcher, // Used when rendering to highlight matches.
}

impl Search {
  pub fn new(notify: Notify) -> Self {
    let mut search =
      Search { index: Index::new(notify.clone()), search: String::new(), cursor: 0, notify };
    search.change_pattern(false);
    search
  }

  pub fn draw(&mut self, render: &mut Render) {
    self.index.nucleo.tick(1);
    let snap = self.index.nucleo.snapshot();

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

    for i in 0..snap.matched_item_count().clamp(0, result_count as u32) {
      let result = snap.get_matched_item(i).unwrap();

      let y = render.size().height - 60.0 - i as f64 * render.store.text.font_metrics().line_height;
      let matched_color = render.theme().search_matched;
      let mut builder =
        render.store.text.layout_builder(result.data, render.theme().text, render.scale());

      let mut indices = vec![];
      self.index.nucleo.pattern.column_pattern(0).indices(
        result.matcher_columns[0].slice_u32(..),
        &mut self.index.matcher,
        &mut indices,
      );

      for i in indices {
        builder
          .apply(i as usize..i as usize + 1, parley::StyleProperty::Brush(matched_color.into()));
        builder.apply(
          i as usize..i as usize + 1,
          parley::StyleProperty::FontWeight(parley::FontWeight::BOLD),
        );
      }

      let (built, backgrounds) = builder.build(result.data);
      let layout = render.build_layout(built, backgrounds);
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

  fn change_pattern(&mut self, append: bool) {
    self.index.nucleo.pattern.reparse(
      0,
      &self.search,
      nucleo::pattern::CaseMatching::Smart,
      nucleo::pattern::Normalization::Smart,
      append,
    );
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { m: Move::Single(Direction::Left), .. } => self.move_cursor(-1),
      Action::Move { m: Move::Single(Direction::Right), .. } => self.move_cursor(1),

      Action::Edit { e: Edit::Insert('\n'), .. } => {
        if let Some(result) = self.index.nucleo.snapshot().get_matched_item(0) {
          self.notify.open_file(PathBuf::from(result.data));
        }
      }
      Action::Edit { e: Edit::Insert(c), .. } => {
        let append = self.cursor == self.search.len();
        self.search.insert(self.cursor, c);
        self.change_pattern(append);
        self.move_cursor(1);
      }
      Action::Edit { e: Edit::Delete(Move::Single(Direction::Right)), .. } => {
        self.delete_graphemes(1);
        self.change_pattern(false);
      }
      Action::Edit { e: Edit::Backspace, .. } => {
        if self.cursor > 0 {
          self.move_cursor(-1);
          self.delete_graphemes(1);
          self.change_pattern(false);
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
  pub fn new(notify: Notify) -> Self {
    let config = nucleo::Config::DEFAULT;
    let nucleo = Nucleo::new(config.clone(), std::sync::Arc::new(move || notify.wake()), None, 1);
    let matcher = nucleo::Matcher::new(config);

    let mut injector = nucleo.injector();
    // TODO: Thread pool.
    std::thread::spawn(move || recurse(".", &mut injector));

    Index { nucleo, matcher }
  }
}

fn recurse(path: &str, injector: &mut Injector<String>) {
  for entry in std::fs::read_dir(path).unwrap() {
    let entry = entry.unwrap();
    let path = entry.path();

    if path.is_dir() {
      recurse(path.to_str().unwrap(), injector);
    } else {
      injector.push(path.to_str().unwrap().to_string(), |path, columns| {
        columns[0] = path.as_str().into();
      });
    }
  }
}
