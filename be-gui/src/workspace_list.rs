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

pub fn draw(render: &mut Render, roots: &[WorkspaceRoot], selected: usize) {
  let size = render.size();
  render.fill(&Rect::from_origin_size(Point::ZERO, size), render.theme().background);

  let line_height = render.store.text.font_metrics(Font::Ui).line_height;
  let row = line_height + 8.0;
  let x = 20.0;

  let title = render.layout_text(Font::Ui, "Workspaces", render.theme().text_dim);
  render.draw_text(&title, Point::new(x, 16.0));

  let top = 16.0 + row + 8.0;
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
