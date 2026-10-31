//! The which-key popup: after a key sequence has been left half typed for a
//! moment, lists the keys that could finish it.

use std::time::Duration;

use be_input::{KeyStroke, Navigation};
use kurbo::{Point, Rect, Size};

use crate::{Font, palette, render::Render};

/// How long a half-typed sequence sits before the popup appears.
pub const DELAY: Duration = Duration::from_millis(300);

const BOTTOM: f64 = 16.0;
const PADDING: f64 = 10.0;
const RADIUS: f64 = 8.0;
const COLUMN_GAP: f64 = 24.0;
const KEY_GAP: f64 = 8.0;
const MAX_ROWS: usize = 12;

/// The keys that could come next after `pending`, and what they do. These are
/// the palette's actions, with the keys bound to them. A key that leads to
/// more than one action, or to a longer bind, is listed as a group.
pub fn entries(
  keybinds: &be_config::Keybinds,
  in_workspace: bool,
  pending: &[KeyStroke],
) -> Vec<(String, String)> {
  let mut actions = palette::actions(in_workspace);
  actions.push(("Command palette", Navigation::CommandPalette));

  let mut out: Vec<(String, String)> = vec![];
  for (name, nav) in actions {
    for bind in keybinds.binds(nav) {
      if bind.len() <= pending.len() || !bind.starts_with(pending) {
        continue;
      }

      let key = bind[pending.len()].label();
      let desc = if bind.len() == pending.len() + 1 { name } else { "+..." };

      match out.iter_mut().find(|(k, _)| *k == key) {
        Some((_, d)) => *d = "+...".to_string(),
        None => out.push((key, desc.to_string())),
      }
    }
  }

  out.sort();
  out
}

/// Draws `entries` (a key and what it does) in the bottom center of the window.
pub fn draw(render: &mut Render, entries: &[(String, String)]) {
  if entries.is_empty() {
    return;
  }

  let window = render.size();
  let line_height = render.store.text.font_metrics(Font::Ui).line_height;
  let row = line_height + 2.0;

  let rows = entries.len().min(MAX_ROWS);
  let columns = entries.len().div_ceil(rows);

  let theme_text = render.theme().text;
  let theme_dim = render.theme().text_dim;

  // Lay out each column, finding how wide it needs to be.
  let mut layouts = vec![];
  let mut key_widths = vec![0.0f64; columns];
  for (i, (key, desc)) in entries.iter().enumerate() {
    let key = render.layout_text(Font::Ui, key, theme_text);
    let desc = render.layout_text(Font::Ui, &desc, theme_dim);
    let col = i / rows;
    key_widths[col] = key_widths[col].max(key.size().width);
    layouts.push((key, desc));
  }
  // A column is as wide as its key column plus its widest description.
  let mut col_widths = vec![0.0f64; columns];
  for (i, (_, desc)) in layouts.iter().enumerate() {
    let col = i / rows;
    col_widths[col] = col_widths[col].max(key_widths[col] + KEY_GAP + desc.size().width);
  }

  let width = col_widths.iter().sum::<f64>() + COLUMN_GAP * (columns - 1) as f64 + PADDING * 2.0;
  let height = rows as f64 * row + PADDING * 2.0;

  let bounds = Rect::from_origin_size(
    Point::new((window.width - width) / 2.0, window.height - BOTTOM - height),
    Size::new(width, height),
  );

  render.clipped(bounds, |render| {
    palette::draw_panel(render, RADIUS);

    let mut x = PADDING;
    for col in 0..columns {
      for (i, (key, desc)) in layouts.iter().enumerate().skip(col * rows).take(rows) {
        let y = PADDING + (i - col * rows) as f64 * row + 1.0;
        render.draw_text(key, Point::new(x, y));
        render.draw_text(desc, Point::new(x + key_widths[col] + KEY_GAP, y));
      }
      x += col_widths[col] + COLUMN_GAP;
    }
  });
}
