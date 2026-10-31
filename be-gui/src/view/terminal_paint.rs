//! Drawing of terminal lines, shared by the terminal view and anything else
//! that renders the contents of a `be_terminal::Emulator`.

use std::{collections::HashMap, ops::Range};

use be_terminal::{BuiltinColor, OwnedLine, Position, StyleFlags, TerminalColor, TerminalState};
use kurbo::{Rect, Size, Stroke};
use parley::FontWeight;
use peniko::color::AlphaColor;

use crate::{Color, Render, RenderStore, TextLayout, oklch, theme::Theme};

/// How many pixels a whole row of `DrawOptions::pixel_offset` is worth. This
/// matches the scroll step used by the terminal view.
const SCROLL_LINE_HEIGHT: f64 = 20.0;

/// How opaque `SGR 2` (dim) text is drawn.
const DIM_ALPHA: f32 = 0.55;

/// The size of a single terminal cell, in logical pixels.
pub fn character_size(store: &RenderStore) -> Size {
  let metrics = store.text.font_metrics(crate::Font::Terminal);
  Size::new(metrics.character_width, metrics.line_height)
}

/// The number of rows and columns of `character` sized cells that fit in
/// `size`.
pub fn grid_size(size: Size, character: Size) -> be_terminal::Size {
  be_terminal::Size {
    rows: (size.height / character.height).floor() as usize,
    cols: (size.width / character.width).floor() as usize,
  }
}

/// Where and how to draw the cursor.
#[derive(Clone, Copy)]
pub struct CursorDraw {
  /// The cursor position, in displayed rows and columns (relative to the
  /// first drawn row, and not affected by `pixel_offset`).
  pub pos:     Position,
  /// Focused cursors are filled, unfocused ones are an outline.
  pub focused: bool,
}

pub struct DrawOptions {
  /// The area to draw into, in the coordinates of `render`. Nothing is
  /// clipped; wrap the call in `Render::clipped` if that is needed.
  pub rect:         Rect,
  /// The index of the first displayed row, counting scrollback followed by the
  /// screen (so 0 is the oldest line of scrollback).
  pub first_row:    usize,
  /// The number of rows to draw, starting at `first_row`.
  pub rows:         usize,
  /// A pixel shift applied to every row. If positive, the row above
  /// `first_row` is drawn as well, as it is partially visible.
  pub pixel_offset: f64,
  /// Selected cells, in displayed rows. Rows above `first_row` are never
  /// selected.
  pub selection:    Option<(Position, Position)>,
  pub cursor:       Option<CursorDraw>,
}

impl DrawOptions {
  /// Draws the live screen of `state`, the way a terminal does: the bottom
  /// `rows` lines, shifted by the viewport scroll.
  pub fn screen(state: &TerminalState, rect: Rect, rows: usize) -> Self {
    DrawOptions {
      rect,
      first_row: state.scrollback_len() - state.viewport_offset().min(state.scrollback_len()),
      rows,
      pixel_offset: 0.0,
      selection: None,
      cursor: None,
    }
  }
}

struct LineLayout {
  layout:     TextLayout,
  background: Vec<(f64, f64, Color)>,
}

struct CachedLine {
  // `OwnedLine::stamp` when the layout was built.
  stamp:     u64,
  // The columns that were drawn selected when the layout was built.
  selection: Option<Range<usize>>,
  // The `frame` this was last drawn in.
  used:      u64,
  line:      LineLayout,
}

/// Draws rows of terminal lines, and caches their layouts.
///
/// The cache is keyed by `OwnedLine::id`, so a painter should only be used
/// with a single emulator. Use one painter per emulator.
#[derive(Default)]
pub struct TerminalPainter {
  // Layouts of lines we have already drawn, by `OwnedLine::id`. Terminal lines keep their id as
  // they scroll, so scrolling and redraws of unchanged lines never rebuild a layout.
  cached_layouts:   HashMap<u64, CachedLine>,
  cached_scale:     f64,
  cached_char_size: Size,
  // Counts calls to `draw`, to drop cache entries that weren't visible.
  frame:            u64,
}

impl TerminalPainter {
  pub fn new() -> Self { TerminalPainter::default() }

  pub fn draw(&mut self, render: &mut Render, state: &TerminalState, options: &DrawOptions) {
    puffin::profile_function!();

    let character_size = character_size(render.store);
    if self.cached_scale != render.scale() || self.cached_char_size != character_size {
      self.cached_layouts.clear();
      self.cached_scale = render.scale();
      self.cached_char_size = character_size;
    }

    self.frame += 1;

    let origin = options.rect.origin();

    let start = if options.pixel_offset > 0.0 { -1 } else { 0 };
    for line in start..options.rows as isize {
      let y = line as f64 * character_size.height + options.pixel_offset * SCROLL_LINE_HEIGHT;
      let Some(layout) = self.layout_line(render, state, options, line) else { continue };

      for range in &layout.background {
        render.fill(
          &Rect::from_origin_size(
            (origin.x + range.0.round(), origin.y + y.round()),
            ((range.1 - range.0).ceil(), character_size.height.ceil()),
          ),
          range.2,
        );
      }
      render.draw_text(&layout.layout, (origin.x, origin.y + y));
    }

    // Forget lines that scrolled out of view, or were dropped from scrollback.
    let frame = self.frame;
    self.cached_layouts.retain(|_, cached| cached.used == frame);

    if let Some(cursor_draw) = options.cursor {
      let pos = cursor_draw.pos;
      let cursor = Rect::from_origin_size(
        (
          origin.x + (pos.col as f64 * character_size.width).round(),
          origin.y + (pos.row as f64 * character_size.height).round(),
        ),
        (character_size.width.ceil(), character_size.height.ceil()),
      );
      if cursor_draw.focused {
        render.fill(&cursor.ceil(), render.theme().text);

        if let Some(line) = display_line(state, options, pos.row as isize) {
          let line_string = line.to_string();
          let offset: usize = line_string.chars().take(pos.col).map(char::len_utf8).sum();
          if let Some(c) = line_string[offset..].chars().next()
            && !c.is_whitespace()
          {
            let mut flags = StyleFlags::empty();
            let mut prev = 0;
            for (style, i) in line.specific_styles(|_, s| s) {
              if offset >= prev && offset < i {
                flags = style.flags;
                break;
              }
              prev = i;
            }

            let text = c.to_string();
            let mut builder = render.store.text.layout_builder(
              crate::Font::Terminal,
              &text,
              render.theme().background,
              render.scale(),
            );
            if flags.contains(StyleFlags::BOLD) {
              builder.apply(0..text.len(), parley::StyleProperty::FontWeight(FontWeight::BLACK));
            }
            if flags.contains(StyleFlags::UNDERLINE) {
              builder.apply(0..text.len(), parley::StyleProperty::Underline(true));
            }
            let (built, backgrounds) = builder.build(&text);
            let layout = render.build_layout(crate::Font::Terminal, built, backgrounds);
            render.draw_text(&layout, (cursor.x0, cursor.y0));
          }
        }
      } else {
        render.stroke(&cursor.inset(-0.5 * render.scale()), render.theme().text, Stroke::new(1.0));
      }
    }
  }

  /// Returns the layout for displayed row `i`, which is negative for the
  /// partially visible line of scrollback above the first row. Layouts are
  /// only rebuilt when the line's contents or selection changed since it was
  /// last drawn.
  fn layout_line(
    &mut self,
    render: &mut Render,
    state: &TerminalState,
    options: &DrawOptions,
    i: isize,
  ) -> Option<&LineLayout> {
    let line = display_line(state, options, i)?;

    let (id, stamp) = (line.id(), line.stamp());
    let selection = selection_cols(options.selection, i);

    let fresh = self
      .cached_layouts
      .get(&id)
      .is_some_and(|cached| cached.stamp == stamp && cached.selection == selection);
    if !fresh {
      let layout = build_line(render, line, selection.as_ref());
      self.cached_layouts.insert(id, CachedLine { stamp, selection, used: 0, line: layout });
    }

    let cached = self.cached_layouts.get_mut(&id).unwrap();
    cached.used = self.frame;
    Some(&cached.line)
  }
}

/// The line drawn at displayed row `i`.
fn display_line<'a>(
  state: &'a TerminalState,
  options: &DrawOptions,
  i: isize,
) -> Option<&'a OwnedLine> {
  state.absolute_line(usize::try_from(options.first_row as isize + i).ok()?)
}

/// The columns of displayed row `row` that are selected.
fn selection_cols(selection: Option<(Position, Position)>, row: isize) -> Option<Range<usize>> {
  if row < 0 {
    return None;
  }
  let row = row as usize;

  let (start, end) = selection?;
  let cols = if row < start.row || row > end.row {
    return None;
  } else if row == start.row && row == end.row {
    start.col..end.col
  } else if row == start.row {
    start.col..usize::MAX
  } else if row == end.row {
    0..end.col
  } else {
    0..usize::MAX
  };

  (!cols.is_empty()).then_some(cols)
}

fn build_line(
  render: &mut Render,
  line: &OwnedLine,
  selection: Option<&Range<usize>>,
) -> LineLayout {
  puffin::profile_function!();

  let inverted_at = |col: usize| selection.is_some_and(|range| range.contains(&col));

  let line_string = line.to_string();

  let theme = &render.store.theme;
  let mut layout = render.store.text.layout_builder(
    crate::Font::Terminal,
    &line_string,
    render.theme().text,
    render.scale(),
  );

  let mut prev = 0;
  for ((foreground, style), i) in line.specific_styles(|j, s| {
    let invert = inverted_at(j) ^ s.flags.contains(StyleFlags::INVERSE);
    let foreground = if invert {
      Some(terminal_color(theme, s.background).unwrap_or(theme.background))
    } else {
      terminal_color(theme, s.foreground)
    };
    (foreground, s)
  }) {
    let foreground = if style.flags.contains(StyleFlags::DIM) {
      Some(foreground.unwrap_or(theme.text).multiply_alpha(DIM_ALPHA))
    } else {
      foreground
    };
    if let Some(foreground) = foreground {
      layout.color_range(prev..i, foreground);
    }
    if style.flags.contains(StyleFlags::BOLD) {
      layout.apply(prev..i, parley::StyleProperty::FontWeight(FontWeight::BLACK));
    }
    if style.flags.contains(StyleFlags::UNDERLINE) {
      layout.apply(prev..i, parley::StyleProperty::Underline(true));
    }
    prev = i;
  }

  let (layout, backgrounds) = layout.build(&line_string);
  let layout = render.build_layout(crate::Font::Terminal, layout, backgrounds);

  let mut background = vec![];
  let mut prev = 0.0;
  for (color, i) in line.specific_styles(|j, s| {
    let invert = inverted_at(j) ^ s.flags.contains(StyleFlags::INVERSE);
    if invert {
      Some(terminal_color(&render.store.theme, s.foreground).unwrap_or(render.store.theme.text))
    } else {
      terminal_color(&render.store.theme, s.background)
    }
  }) {
    let x = layout.cursor(i, crate::CursorMode::Line).x0;
    if let Some(color) = color {
      background.push((prev, x, color));
    }
    prev = x;
  }

  LineLayout { layout, background }
}

/// The color a terminal draws the (non-bright) ANSI color `color` in.
pub fn ansi_color(color: BuiltinColor) -> Color {
  use BuiltinColor::*;

  match color {
    Black => oklch(0.6, 0.0, 0.0),
    Red => oklch(0.75, 0.13, 25.0),
    Green => oklch(0.8, 0.14, 140.0),
    Yellow => oklch(0.95, 0.12, 85.0),
    Blue => oklch(0.8, 0.12, 240.0),
    Magenta => oklch(0.8, 0.13, 350.0),
    Cyan => oklch(0.85, 0.1, 200.0),
    White => oklch(1.0, 0.0, 0.0),
  }
}

fn terminal_color(_theme: &Theme, color: Option<TerminalColor>) -> Option<Color> {
  Some(match color {
    Some(TerminalColor::Builtin { color, bright: _ }) => ansi_color(color),
    Some(TerminalColor::Rgb { r, g, b }) => AlphaColor::from_rgb8(r, g, b).convert(),
    _ => return None,
  })
}

/// A piece of text in a line built by [`styled_line`].
pub struct Span<'a> {
  pub text:  &'a str,
  /// `None` is the theme's text color.
  pub color: Option<Color>,
  pub bold:  bool,
}

/// Lays out one line of terminal-font text with per-span color and weight,
/// the same way [`TerminalPainter`] does for terminal lines, so the glyphs
/// line up with the cells of terminal output.
pub fn styled_line(render: &mut Render, spans: &[Span]) -> TextLayout {
  let line: String = spans.iter().map(|s| s.text).collect();
  let mut layout = render.store.text.layout_builder(
    crate::Font::Terminal,
    &line,
    render.theme().text,
    render.scale(),
  );

  let mut start = 0;
  for span in spans {
    let end = start + span.text.len();
    if let Some(color) = span.color {
      layout.color_range(start..end, color);
    }
    if span.bold {
      layout.apply(start..end, parley::StyleProperty::FontWeight(FontWeight::BLACK));
    }
    start = end;
  }

  let (layout, backgrounds) = layout.build(&line);
  render.build_layout(crate::Font::Terminal, layout, backgrounds)
}
