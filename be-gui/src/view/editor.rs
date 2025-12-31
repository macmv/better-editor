use std::collections::HashMap;

use be_doc::crop::RopeSlice;
use be_editor::EditorState;
use be_input::Mode;
use kurbo::{Line, Point, Rect, RoundedRect, Stroke, Vec2};

use crate::{CursorMode, Render, RenderStore, TextLayout, theme::Underline};

pub struct EditorView {
  pub editor: EditorState,

  scroll:  Point,
  focused: bool,

  cached_layouts: HashMap<usize, TextLayout>,
  cached_scale:   f64,
}

impl EditorView {
  pub fn new(store: &RenderStore) -> Self {
    let mut view = EditorView {
      editor:         EditorState::from("💖hello\n💖foobar\nsdjkhfl\nworld\n"),
      scroll:         Point::ZERO,
      focused:        true,
      cached_layouts: HashMap::new(),
      cached_scale:   0.0,
    };

    view.editor.config = store.config.clone();
    view.editor.lsp.store = store.lsp.clone();

    view
  }

  pub fn on_focus(&mut self, focus: bool) { self.focused = focus; }

  pub fn draw(&mut self, render: &mut Render) {
    if self.cached_scale != render.scale() {
      self.cached_layouts.clear();
      self.cached_scale = render.scale();
    }

    if self.editor.take_damage_all() {
      self.cached_layouts.clear();
    }

    for line in self.editor.take_damages() {
      self.cached_layouts.remove(&line.as_usize());
    }

    self.editor.update_diagnostics();

    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background,
    );

    let line_height = render.store.text.font_metrics().line_height;

    let scroll_offset = self.editor.config.borrow().editor.scroll_offset as usize;

    let min_fully_visible_row = (self.scroll.y / line_height).ceil() as usize + scroll_offset;
    let max_fully_visible_row =
      ((self.scroll.y + render.size().height) / line_height).floor() as usize - 1 - scroll_offset;

    if self.editor.cursor().line.as_usize() < min_fully_visible_row {
      let target_line = self
        .editor
        .cursor()
        .line
        .as_usize()
        .saturating_sub(scroll_offset)
        .clamp(0, self.editor.doc().rope.lines().len());

      self.scroll.y = target_line as f64 * line_height;
    } else if self.editor.cursor().line.as_usize() > max_fully_visible_row {
      let target_line = self
        .editor
        .cursor()
        .line
        .as_usize()
        .saturating_add(scroll_offset + 1)
        .clamp(0, self.editor.doc().rope.lines().len());

      self.scroll.y = (target_line as f64 * line_height) - render.size().height;
    }

    let min_line = ((self.scroll.y / line_height).floor() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());
    let max_line = (((self.scroll.y + render.size().height) / line_height).ceil() as usize)
      .clamp(0, self.editor.doc().rope.lines().len());

    let start = self.editor.doc().rope.byte_of_line(min_line);
    let end = if max_line >= self.editor.doc().rope.line_len() {
      self.editor.doc().rope.byte_len()
    } else {
      self.editor.doc().rope.byte_of_line(max_line + 1)
    };

    let mut index = start;
    let mut i = min_line;

    let start_y = -(self.scroll.y % line_height);
    let mut y = start_y;
    let mut indent_guides =
      IndentGuides::new(self.editor.config.borrow().editor.indent_width as usize, y);
    loop {
      if self.layout_line(render, i, index).is_none() {
        break;
      };
      indent_guides
        .visit(self.editor.doc().rope.byte_slice(index..).raw_lines().next().unwrap(), render);

      let layout = self.cached_layouts.get(&i).unwrap();

      render.draw_text(&layout, Point::new(20.0, y));

      y += line_height;
      i += 1;
      index += self.editor.doc().rope.byte_slice(index..).raw_lines().next().unwrap().byte_len();
      if index >= end {
        break;
      }
    }

    indent_guides.finish(render);

    if self.focused {
      let mode = match self.editor.mode() {
        Mode::Normal | Mode::Visual => Some(CursorMode::Block),
        Mode::Insert => Some(CursorMode::Line),
        Mode::Replace => Some(CursorMode::Underline),
        Mode::Command => None,
      };

      if let Some(mode) = mode {
        let line = self.editor.cursor().line.as_usize();
        let layout = &self.cached_layouts[&line];

        let cursor = layout
          .cursor(self.editor.doc().cursor_column_offset(self.editor.cursor()), mode)
          + Vec2::new(20.0, start_y + (line - min_line) as f64 * line_height);
        render.fill(&cursor, render.theme().text);

        if let Some(completions) = self.editor.completions() {
          let layouts = completions
            .iter()
            .take(20)
            .map(|completion| render.layout_text(&completion, render.theme().text))
            .collect::<Vec<_>>();

          let inner_width = layouts
            .iter()
            .map(|layout| layout.size().width)
            .max_by(|a, b| a.total_cmp(b))
            .unwrap_or(0.0);
          let inner_height = layouts.len() as f64 * line_height;

          const MARGIN_X: f64 = 10.0;
          const MARGIN_Y: f64 = 5.0;

          let start_x = cursor.x0;
          let start_y;
          let mut y;
          let rect;

          if cursor.y1 + inner_height + MARGIN_Y * 2.0 > render.size().height {
            // draw above the cursor
            start_y = cursor.y0;
            y = start_y - inner_height - MARGIN_Y;

            rect = Rect::new(
              start_x - MARGIN_X,
              start_y - inner_height - MARGIN_Y * 2.0,
              start_x + inner_width + MARGIN_X,
              start_y,
            );
          } else {
            // draw below the cursor
            start_y = cursor.y1;
            y = start_y + MARGIN_Y;

            rect = Rect::new(
              start_x - MARGIN_X,
              start_y,
              start_x + inner_width + MARGIN_X,
              start_y + inner_height + MARGIN_Y * 2.0,
            );
          }

          render.drop_shadow(
            rect,
            MARGIN_Y,
            2.0,
            // keep the chroma and hue so they blend nicely.
            render.theme().background.map(|_, c, h, _| [0.0, c, h, 0.2]),
          );
          render.fill(&RoundedRect::from_rect(rect, MARGIN_Y), render.theme().background_raised);

          for layout in layouts {
            render.draw_text(&layout, (start_x, y));
            y += line_height;
          }
        }
      }
    }

    if let Some(command) = self.editor.command() {
      render.fill(
        &Rect::new(
          0.0,
          render.size().height - line_height,
          render.size().width,
          render.size().height,
        ),
        render.theme().background_raised,
      );

      let text_pos = Point::new(20.0, render.size().height - line_height);

      let layout = render.layout_text(&command.text, render.theme().text);
      render.draw_text(&layout, text_pos);

      let cursor = layout.cursor(command.cursor as usize, CursorMode::Line);
      render.fill(&(cursor + text_pos.to_vec2()), render.theme().text);
    } else if let Some(status) = self.editor.status() {
      render.fill(
        &Rect::new(
          0.0,
          render.size().height - line_height,
          render.size().width,
          render.size().height,
        ),
        render.theme().background_raised,
      );

      let layout = render.layout_text(&status.message, render.theme().text);
      render.draw_text(&layout, (20.0, render.size().height - line_height));
    }

    if let Some(ft) = self.editor.file_type() {
      let layout = render.layout_text(&format!("{ft}"), render.theme().text);
      render.draw_text(&layout, (render.size().width - 50.0, render.size().height - line_height));
    }
  }

  fn layout_line(
    &mut self,
    render: &mut Render,
    i: usize,
    index: usize,
  ) -> Option<&mut TextLayout> {
    let entry = match self.cached_layouts.entry(i) {
      std::collections::hash_map::Entry::Occupied(entry) => return Some(entry.into_mut()),
      std::collections::hash_map::Entry::Vacant(entry) => entry,
    };

    let line = self.editor.doc().rope.byte_slice(index..).raw_lines().next()?;
    let max_index = index + line.byte_len();

    let line_string = line.to_string();
    let theme = &render.store.theme;
    let mut layout =
      render.store.text.layout_builder(&line_string, render.theme().text, render.scale());

    let highlights = self.editor.highlights(index..max_index);
    let mut prev = index;
    for highlight in highlights {
      let pos = if highlight.pos > max_index { max_index } else { highlight.pos };

      if let Some(highlight) = theme.syntax.lookup(&highlight.highlights) {
        let range = prev - index..pos - index;
        if let Some(foreground) = highlight.foreground {
          layout.color_range(range.clone(), foreground);
        }
        if let Some(weight) = highlight.weight {
          layout.apply(range.clone(), parley::StyleProperty::FontWeight(weight.to_parley()));
        }
        if let Some(underline) = highlight.underline {
          layout.apply(range.clone(), parley::StyleProperty::Underline(true));

          if let Underline::Color(c) = underline {
            layout.apply(range.clone(), parley::StyleProperty::UnderlineBrush(Some(c.into())));
          }
        }
      }

      if highlight.pos > max_index {
        break;
      }

      prev = highlight.pos;
    }

    let layout = layout.build(&line_string);
    let layout = render.build_layout(layout);

    Some(entry.insert(layout))
  }
}

struct IndentGuides {
  indent_width:  usize,
  scroll_offset: f64,

  starts:       Vec<usize>,
  current_line: usize,
}

impl IndentGuides {
  pub fn new(indent_width: usize, scroll_offset: f64) -> Self {
    IndentGuides { indent_width, scroll_offset, starts: vec![], current_line: 0 }
  }

  pub fn visit(&mut self, line: RopeSlice, render: &mut Render) {
    if line.chars().all(|c| c.is_whitespace()) {
      self.current_line += 1;
      return;
    }

    let indent = line.chars().take_while(|c| *c == ' ').count() / self.indent_width;

    while self.starts.len() > indent {
      let start = self.starts.pop().unwrap();
      self.draw_line(start, self.current_line, render);
    }

    while self.starts.len() < indent {
      self.starts.push(self.current_line);
    }

    self.current_line += 1;
  }

  pub fn finish(&mut self, render: &mut Render) {
    while let Some(start) = self.starts.pop() {
      self.draw_line(start, self.current_line, render);
    }
  }

  fn draw_line(&self, start: usize, end: usize, render: &mut Render) {
    const INDENT_GUIDE_WIDTH: f64 = 1.0;
    const INDENT_GUIDE_END_OFFSET: f64 = 2.0;

    let x = self.starts.len() as f64
      * render.store.text.font_metrics().character_width
      * self.indent_width as f64
      + 20.0
      + INDENT_GUIDE_WIDTH / 2.0;
    let min_y = start as f64 * render.store.text.font_metrics().line_height + self.scroll_offset;
    let max_y = end as f64 * render.store.text.font_metrics().line_height + self.scroll_offset
      - INDENT_GUIDE_END_OFFSET;

    render.stroke(
      &Line::new((x, min_y), (x, max_y)),
      render.theme().background_raised,
      Stroke::new(INDENT_GUIDE_WIDTH),
    );
  }
}
