//! The screen listing every open workspace.

use be_fs::WorkspaceRoot;
use kurbo::{Point, Rect};

use crate::{Font, render::Render};

/// How a workspace is named in the list: `host:/path` if it is on another
/// machine, and `/path` if it isn't.
pub fn label(root: &WorkspaceRoot) -> String {
  match root.host() {
    Some(host) => format!("{host}:{}", root.as_path().display()),
    None => root.as_path().display().to_string(),
  }
}

/// Orders the roots for display. Local workspaces come first, then remote ones
/// grouped by host.
pub fn sort(roots: &mut [WorkspaceRoot]) {
  roots.sort_by(|a, b| (a.host(), a.as_path()).cmp(&(b.host(), b.as_path())));
}

const X: f64 = 20.0;

/// Where the rows start, and how tall each is.
fn rows(line_height: f64) -> (f64, f64) {
  let row = line_height + 8.0;
  (16.0 + row + 8.0, row)
}

/// The row of `count` under `pos`, if any.
pub fn row_at(pos: Point, line_height: f64, count: usize) -> Option<usize> {
  let (top, row) = rows(line_height);
  if pos.y < top || pos.x < X - 10.0 {
    return None;
  }
  let i = ((pos.y - top) / row) as usize;
  (i < count).then_some(i)
}

pub fn draw(render: &mut Render, roots: &[WorkspaceRoot], selected: usize) {
  let size = render.size();
  render.fill(&Rect::from_origin_size(Point::ZERO, size), render.theme().background);

  let line_height = render.store.text.font_metrics(Font::Ui).line_height;
  let (top, row) = rows(line_height);
  let x = X;

  let title = render.layout_text(Font::Ui, "Workspaces", render.theme().text_dim);
  render.draw_text(&title, Point::new(x, 16.0));

  if roots.is_empty() {
    let none = render.layout_text(Font::Ui, "No workspaces are open", render.theme().text_dim);
    render.draw_text(&none, Point::new(x, top));
    return;
  }

  for (i, root) in roots.iter().enumerate() {
    let y = top + i as f64 * row;
    if y > size.height {
      break;
    }

    if i == selected {
      let bounds = Rect::new(x - 10.0, y, size.width - x + 10.0, y + row);
      render.fill(&bounds, render.theme().background_raised);
    }

    let layout = render.layout_text(Font::Ui, &label(root), render.theme().text);
    render.draw_text(&layout, Point::new(x, y + 4.0));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rows_are_found_by_position() {
    let (top, row) = rows(16.0);

    assert_eq!(row_at(Point::new(30.0, top - 1.0), 16.0, 3), None);
    assert_eq!(row_at(Point::new(30.0, top + 1.0), 16.0, 3), Some(0));
    assert_eq!(row_at(Point::new(30.0, top + row + 1.0), 16.0, 3), Some(1));
    assert_eq!(row_at(Point::new(30.0, top + 3.0 * row + 1.0), 16.0, 3), None);
    assert_eq!(row_at(Point::new(2.0, top + 1.0), 16.0, 3), None);
  }
}
