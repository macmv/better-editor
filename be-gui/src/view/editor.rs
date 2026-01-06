use std::collections::HashMap;

use be_animation::Animation;
use be_editor::{CommandMode, EditorState, IndentLevel};
use be_input::Mode;
use kurbo::{Arc, Circle, Line, Point, Rect, RoundedRect, Stroke, Triangle, Vec2};

use crate::{CursorMode, Render, RenderStore, TextLayout, theme::Underline};

pub struct EditorView {
  pub editor: EditorState,

  scroll:  Point,
  focused: bool,

  // This is kinda hacky but ah well.
  pub(crate) temporary_replace_mode: bool,
  cached_layouts:                    HashMap<usize, TextLayout>,
  cached_scale:                      f64,

  progress_animation: Animation,
}

impl EditorView {
  pub fn new(store: &RenderStore) -> Self {
    let mut view = EditorView {
      editor:                 EditorState::from("💖hello\n💖foobar\nsdjkhfl\nworld\n"),
      scroll:                 Point::ZERO,
      focused:                true,
      temporary_replace_mode: false,
      cached_layouts:         HashMap::new(),
      cached_scale:           0.0,

      progress_animation: Animation::linear(2.0),
    };

    view.editor.config = store.config.clone();
    view.editor.lsp.store = store.lsp.clone();
    let notifier = store.notifier();
    view.editor.exit_cmd = Some(Box::new(move || {
      notifier.exit();
    }));

    view
  }

  pub fn on_focus(&mut self, focus: bool) { self.focused = focus; }

  pub fn draw(&mut self, render: &mut Render) {
    self.editor.update();

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

    let min_line = be_doc::Line(
      ((self.scroll.y / line_height).floor() as usize)
        .clamp(0, self.editor.doc().rope.lines().len()),
    );
    let max_line = be_doc::Line(
      (((self.scroll.y + render.size().height) / line_height).ceil() as usize)
        .clamp(0, self.editor.doc().rope.lines().len()),
    );

    let start = self.editor.doc().byte_of_line(min_line);
    let end = if max_line.as_usize() >= self.editor.doc().len_lines() {
      self.editor.doc().rope.byte_len()
    } else {
      self.editor.doc().byte_of_line(max_line + 1)
    };

    let mut index = start;
    let mut i = min_line.as_usize();

    let start_y = -(self.scroll.y % line_height);
    let mut y = start_y;
    let mut indent_guides =
      IndentGuides::new(self.editor.config.borrow().editor.indent_width as usize, y);
    loop {
      if self.layout_line(render, i, index).is_none() {
        break;
      };
      indent_guides
        .visit(self.editor.guess_indent(be_doc::Line(i), be_input::VerticalDirection::Up), render);

      let layout = self.cached_layouts.get(&i).unwrap();
      render.draw_text(&layout, Point::new(20.0, y));

      let mut shape =
        Circle::new((0.0, y + render.store.text.font_metrics().line_height / 2.0), 1.5);
      for (i, c) in self.editor.doc().line(be_doc::Line(i)).chars().rev().enumerate() {
        if c == ' ' {
          shape.center.x = 20.0 + layout.size().width
            - (i as f64 * render.store.text.font_metrics().character_width)
            - render.store.text.font_metrics().character_width / 2.0;
          render.fill(&shape, render.theme().background_raised);
        } else {
          break;
        }
      }

      y += line_height;
      i += 1;
      index += self.editor.doc().rope.byte_slice(index..).raw_lines().next().unwrap().byte_len();
      if index >= end {
        break;
      }
    }

    if let Some(changes) = &self.editor.changes {
      for hunk in changes.hunks() {
        if be_doc::Line(hunk.after.end) < min_line || be_doc::Line(hunk.after.start) > max_line {
          continue;
        }

        for change in hunk.changes.iter().rev() {
          if be_doc::Line(change.after().end) < min_line
            || be_doc::Line(change.after().start) > max_line
          {
            continue;
          }

          if change.after().is_empty() {
            let y = start_y + (change.after().start - min_line.as_usize()) as f64 * line_height;

            let shape = Triangle::new((0.0, y - 4.0), (0.0, y + 4.0), (4.0, y));
            render.fill(&shape, render.theme().diff_remove);
          } else {
            let min_y =
              start_y + (change.after().start as f64 - min_line.as_usize() as f64) * line_height;
            let max_y =
              start_y + (change.after().end as f64 - min_line.as_usize() as f64) * line_height;

            let shape = Rect::new(0.0, min_y, 4.0, max_y);
            render.fill(
              &shape,
              if change.before().is_empty() {
                render.theme().diff_add
              } else {
                render.theme().diff_change
              },
            );
          }
        }
      }
    }

    indent_guides.finish(render);

    if self.focused {
      let mode = match self.editor.mode() {
        Mode::Normal if self.temporary_replace_mode => Some(CursorMode::Underline),
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
          + Vec2::new(20.0, start_y + (line - min_line.as_usize()) as f64 * line_height);
        render.fill(&cursor.ceil(), render.theme().text);

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

    render.fill(
      &Rect::new(
        0.0,
        render.size().height - line_height,
        render.size().width,
        render.size().height,
      ),
      render.theme().background_raised,
    );

    if let Some(command) = self.editor.command() {
      let text_pos = Point::new(20.0, render.size().height - line_height);

      let text = format!(
        "{}{}",
        match command.mode {
          CommandMode::Command => ":",
          CommandMode::Search => "/",
        },
        command.text
      );

      let layout = render.layout_text(&text, render.theme().text);
      render.draw_text(&layout, text_pos);

      let cursor = layout.cursor(command.cursor as usize + 1, CursorMode::Line);
      render.fill(&(cursor + text_pos.to_vec2()), render.theme().text);
    } else if let Some(status) = self.editor.status() {
      let layout = render.layout_text(&status.message, render.theme().text);
      render.draw_text(&layout, (20.0, render.size().height - line_height));
    }

    if let Some(ft) = self.editor.file_type() {
      let layout = render.layout_text(&format!("{ft}"), render.theme().text);
      render.draw_text(&layout, (render.size().width - 50.0, render.size().height - line_height));
    }

    let progress = self.editor.progress();
    if progress.is_empty() {
      self.progress_animation.stop();
    } else if !self.progress_animation.is_running() {
      self.progress_animation.start();
    }
    self.progress_animation.advance(render.now());
    if self.progress_animation.is_running() {
      let arc = Arc::new(
        (10.0, render.size().height - 10.0),
        (8.0, 8.0),
        0.0,
        std::f64::consts::PI * 4.0 / 3.0,
        self.progress_animation.interpolate(0.0, std::f64::consts::PI * 2.0),
      );
      render.stroke(&arc, crate::oklch(1.0, 0.0, 0.0), Stroke::new(1.0));
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
      let mut pos = if highlight.pos > max_index { max_index } else { highlight.pos };

      if pos < index || pos <= prev {
        continue;
      }

      // Round up to char boundaries. Avoids panics when laying out text. It's still
      // wrong, but highlights come from places like LSP, where we can't trust
      // their positions.
      while !line_string.is_char_boundary(pos - index) && pos < max_index {
        pos += 1;
      }

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
        if let Some(background) = highlight.background {
          layout.background(range.clone(), background);
        }
      }

      if highlight.pos > max_index {
        break;
      }

      prev = pos;
    }

    let (layout, backgrounds) = layout.build(&line_string);
    let layout = render.build_layout(layout, backgrounds);

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

  pub fn visit(&mut self, level: IndentLevel, render: &mut Render) {
    while self.starts.len() > level.0 {
      let start = self.starts.pop().unwrap();
      self.draw_line(start, self.current_line, render);
    }

    while self.starts.len() < level.0 {
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
    const INDENT_GUIDE_START_OFFSET: f64 = 5.0;
    const INDENT_GUIDE_END_OFFSET: f64 = 0.0;

    let x = (self.starts.len() as f64
      * render.store.text.font_metrics().character_width
      * self.indent_width as f64)
      .round()
      + 20.0
      + INDENT_GUIDE_WIDTH / 2.0;
    let min_y = start as f64 * render.store.text.font_metrics().line_height
      + self.scroll_offset
      + INDENT_GUIDE_START_OFFSET;
    let max_y = end as f64 * render.store.text.font_metrics().line_height + self.scroll_offset
      - INDENT_GUIDE_END_OFFSET;

    render.stroke(
      &Line::new((x, min_y), (x, max_y)),
      render.theme().background_raised,
      Stroke::new(INDENT_GUIDE_WIDTH),
    );
  }
}
