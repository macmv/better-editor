use std::{collections::HashMap, io, path::PathBuf};

use be_animation::Animation;
use be_doc::Cursor;
use be_editor::{CommandMode, EditorEvent, EditorState, IndentLevel};
use be_input::{Action, Mode};
use be_shared::SharedHandle;
use be_workspace::Workspace;
use kurbo::{Arc, Circle, Line, Point, Rect, RoundedRect, Size, Stroke, Triangle, Vec2};

use crate::{
  CursorMode, Layout, MouseButton, MouseEvent, Render, RenderStore, TextLayout, theme::Underline,
};

pub struct EditorView {
  pub editor: SharedHandle<EditorState>,

  scroll: Point,
  focus:  Focus,

  // This is kinda hacky but ah well.
  pub(crate) temporary_underline: bool,
  cached_layouts:                 HashMap<usize, TextLayout>,
  cached_scale:                   f64,

  line_numbers:      Vec<TextLayout>,
  line_number_width: f64,

  /// The minimum visible line. This will always be a real line in the file. It
  /// may only be partially visible.
  min_line: be_doc::Line,
  /// The maximum visible line. This might not be a real line in the file, if
  /// the file is too short, or the user has scrolled down. It may only be
  /// partially visible.
  max_line: be_doc::Line,

  definition_history: Vec<(Cursor, PathBuf)>,

  progress_animation: Animation,
}

enum Focus {
  Focused,
  Unfocused { cursor: Cursor },
}

const LINE_NUMBER_MARGIN_LEFT: f64 = 10.0;
const LINE_NUMBER_MARGIN_RIGHT: f64 = 10.0;

impl EditorView {
  pub fn new(store: &mut RenderStore) -> Self {
    let mut view = EditorView {
      editor:              store.workspace.new_editor(),
      scroll:              Point::ZERO,
      focus:               Focus::Unfocused { cursor: Cursor::default() },
      temporary_underline: false,
      cached_layouts:      HashMap::new(),
      cached_scale:        0.0,

      min_line:          be_doc::Line(0),
      max_line:          be_doc::Line(0),
      line_numbers:      vec![],
      line_number_width: 0.0,

      definition_history: vec![],

      progress_animation: Animation::linear(2.0),
    };

    view.progress_animation.set_repeat(true);

    view
  }

  pub fn cursor(&self) -> Cursor {
    match self.focus {
      Focus::Focused => self.editor.cursor(),
      Focus::Unfocused { cursor } => cursor,
    }
  }
  pub fn doc(&self) -> &be_doc::Document { self.editor.doc() }

  pub fn split_from(&mut self, editor: &EditorView) {
    // TODO: Save if unsaved.
    self.editor = editor.editor.clone();
  }

  pub fn open(&mut self, path: &std::path::Path, workspace: &mut Workspace) -> io::Result<()> {
    self.editor = workspace.open_file(path)?;
    Ok(())
  }

  pub fn animated(&self) -> bool { self.progress_animation.is_running() }

  pub fn layout(&mut self, layout: &mut Layout) {
    puffin::profile_function!();

    if self.cached_scale != layout.scale() {
      self.cached_layouts.clear();
      self.cached_scale = layout.scale();
    }

    if self.editor.is_damage_all() {
      self.cached_layouts.clear();
    }

    for line in self.editor.damages() {
      self.cached_layouts.remove(&line.as_usize());
    }

    let line_height = layout.store.text.font_metrics().line_height;

    layout.split(
      self,
      kurbo::Axis::Horizontal,
      crate::Distance::Pixels(-line_height),
      |state, layout| state.layout_editor(layout),
      |_, _| {},
    );
  }

  pub fn draw(&mut self, render: &mut Render) {
    puffin::profile_function!();

    let line_height = render.store.text.font_metrics().line_height;

    render.split(
      self,
      kurbo::Axis::Horizontal,
      crate::Distance::Pixels(-line_height),
      |state, render| state.draw_editor(render),
      |state, render| state.draw_status(render),
    );

    self.draw_progress(render);
  }

  pub fn on_focus(&mut self, focus: bool) {
    if focus {
      if let Focus::Unfocused { cursor } = self.focus {
        self.editor.move_to(cursor);
      }

      self.focus = Focus::Focused;
    } else {
      self.focus = Focus::Unfocused { cursor: self.editor.cursor() };
    }
  }

  fn focused(&self) -> bool { matches!(self.focus, Focus::Focused) }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { count: _, m: be_input::Move::BackDefinition } => {
        if let Some((cursor, path)) = self.definition_history.pop() {
          if let Some(file) = self.editor.file()
            && *file == path
          {
            self.editor.move_to(cursor);
          } else {
            if let Some(send) = &self.editor.send {
              send(EditorEvent::OpenFile(path, Some(cursor)));
            }
          }
        }
      }

      _ => self.editor.perform_action(action),
    }
  }

  pub fn record_definition(&mut self, path: PathBuf, cursor: Cursor) {
    self.definition_history.push((cursor, path));
  }

  pub fn on_mouse(
    &mut self,
    ev: &crate::MouseEvent,
    size: Size,
    store: &RenderStore,
  ) -> crate::CursorKind {
    let line_height = store.text.font_metrics().line_height;

    match ev {
      MouseEvent::Move { pos } => {
        if pos.y >= size.height - line_height {
          // status bar
        } else {
          if pos.x >= self.gutter_width() {
            return crate::CursorKind::Beam;
          } else {
            let Some(line) = self.line_for_mouse(store, pos.y) else {
              return crate::CursorKind::Default;
            };

            if pos.x < 4.0
              && let Some(()) = self.change_gutter_for_line(line)
            {
              // TODO: Show the gutter in a popup when clicked.
              return crate::CursorKind::Pointer;
            }
          }
        }
      }

      MouseEvent::Button { pos, pressed: true, button: MouseButton::Left } => {
        if pos.y >= size.height - line_height {
          // status bar
        } else {
          let line = self
            .line_for_mouse(store, pos.y)
            .unwrap_or_else(|| be_doc::Line(self.doc().rope.lines().len().saturating_sub(1)));
          let Some(layout) = self.cached_layouts.get(&line.0) else {
            return crate::CursorKind::Default;
          };

          let Some(cursor_mode) = self.cursor_mode() else {
            return crate::CursorKind::Default;
          };

          if pos.x >= self.gutter_width() {
            let column_byte = layout.index(pos.x - self.gutter_width(), cursor_mode);
            let column = self.doc().line(line).byte_slice(..column_byte).graphemes().count();

            self.editor.move_to(Cursor {
              line,
              column: be_doc::Column(column),
              target_column: be_doc::VisualColumn(0), // NB: Replaced in `move_to`.
            });
          }
        }
      }

      MouseEvent::Scroll { pos, delta } => {
        if pos.y >= size.height - line_height {
          // status bar
        } else {
          let size = Size::new(size.width, size.height - line_height);

          self.scroll.y = (self.scroll.y - delta.y).max(0.0);

          if self.focused() {
            let scroll_offset = self.editor.config.borrow().settings.editor.scroll_offset as usize;

            let min_fully_visible_row =
              (self.scroll.y / line_height).ceil() as usize + scroll_offset;
            let max_fully_visible_row =
              ((self.scroll.y + size.height) / line_height).floor() as usize - 1 - scroll_offset;

            if self.cursor().line.as_usize() < min_fully_visible_row {
              self.editor.move_to_line(be_doc::Line(min_fully_visible_row));
            } else if self.cursor().line.as_usize() > max_fully_visible_row {
              self.editor.move_to_line(be_doc::Line(max_fully_visible_row));
            }
          }
        }
      }

      _ => {}
    }

    crate::CursorKind::Default
  }

  fn line_for_mouse(&self, store: &RenderStore, y: f64) -> Option<be_doc::Line> {
    let line_height = store.text.font_metrics().line_height;

    let line_region_y = self.scroll.y + y;
    let line = (line_region_y / line_height).floor() as usize;
    if line < self.doc().rope.lines().len() { Some(be_doc::Line(line)) } else { None }
  }

  fn layout_editor(&mut self, layout: &mut Layout) {
    let line_height = layout.store.text.font_metrics().line_height;
    let scroll_offset = self.editor.config.borrow().settings.editor.scroll_offset as usize;

    if self.focused() {
      let min_fully_visible_row = (self.scroll.y / line_height).ceil() as usize + scroll_offset;
      let max_fully_visible_row =
        ((self.scroll.y + layout.size().height) / line_height).floor() as usize - 1 - scroll_offset;

      if self.cursor().line.as_usize() < min_fully_visible_row {
        let target_line = self
          .cursor()
          .line
          .as_usize()
          .saturating_sub(scroll_offset)
          .clamp(0, self.doc().rope.lines().len());

        self.scroll.y = target_line as f64 * line_height;
      } else if self.cursor().line.as_usize() > max_fully_visible_row {
        let target_line = self
          .cursor()
          .line
          .as_usize()
          .saturating_add(scroll_offset + 1)
          .clamp(0, self.doc().rope.lines().len());

        self.scroll.y = (target_line as f64 * line_height) - layout.size().height;
      }
    }

    self.min_line = be_doc::Line(
      ((self.scroll.y / line_height).floor() as usize)
        .clamp(0, self.doc().rope.lines().len().saturating_sub(1)),
    );
    self.max_line = be_doc::Line(
      (((self.scroll.y + layout.size().height) / line_height).ceil() as usize)
        .clamp(0, self.doc().rope.lines().len().saturating_sub(1)),
    );

    let start = self.doc().byte_of_line(self.min_line);
    let end = if self.max_line.as_usize() >= self.doc().len_lines() {
      self.doc().rope.byte_len()
    } else {
      self.doc().byte_of_line(self.max_line + 1)
    };

    let mut index = start;
    let mut i = self.min_line.as_usize();

    self.line_numbers.clear();
    // Layout the length line number by default. If `character_width` is wrong, then
    // we'll still take the `max()` below.
    self.line_number_width = layout.store.text.font_metrics().character_width
      * ((self.doc().len_lines() as f64).log10().floor() + 1.0);

    while index < end {
      if self.layout_line(layout, i, index).is_none() {
        break;
      };

      let color = if self.focused() && self.cursor().line.as_usize() == i {
        layout.theme().text
      } else {
        layout.theme().text_dim
      };

      let line_number_text = (i + 1).to_string();
      let layout = layout.layout_text(&line_number_text, color);
      self.line_number_width = self.line_number_width.max(layout.size().width);
      self.line_numbers.push(layout);

      i += 1;
      index += self.doc().rope.byte_slice(index..).raw_lines().next().unwrap().byte_len();
    }
  }

  fn draw_editor(&mut self, render: &mut Render) {
    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background,
    );

    let line_height = render.store.text.font_metrics().line_height;

    let start_y = -(self.scroll.y % line_height);

    let mut y = start_y;
    let mut indent_guides = IndentGuides::new(
      self.editor.config.borrow().settings.editor.indent_width as usize,
      start_y,
      self.gutter_width(),
    );
    for i in self.min_line.as_usize()..=self.max_line.as_usize() {
      if self.cached_layouts.get(&i).is_none() {
        break;
      }

      indent_guides
        .visit(self.editor.guess_indent(be_doc::Line(i), be_input::VerticalDirection::Up), render);

      let layout = &self.line_numbers[i - self.min_line.as_usize()];
      render.draw_text(
        layout,
        Point::new(LINE_NUMBER_MARGIN_LEFT + self.line_number_width - layout.size().width, y),
      );

      let layout = self.cached_layouts.get(&i).unwrap();
      render.draw_text(&layout, Point::new(self.gutter_width(), y));

      self.draw_trailing_spaces(i, self.gutter_width() + layout.size().width, y, render);

      y += line_height;
    }

    self.draw_change_gutter(start_y, render);

    indent_guides.finish(render);

    if let Some(mode) = self.cursor_mode() {
      let line = self.cursor().line.as_usize();
      let layout = &self.cached_layouts[&line];

      if line >= self.min_line.as_usize() && line <= self.max_line.as_usize() {
        let cursor = layout.cursor(self.doc().cursor_column_offset(self.cursor()), mode)
          + Vec2::new(
            self.gutter_width(),
            start_y + (line - self.min_line.as_usize()) as f64 * line_height,
          );
        if self.focused() {
          render.fill(&cursor.ceil(), render.theme().text);
        } else {
          render.stroke(
            &cursor.inset(-0.5 * render.scale()),
            render.theme().text,
            Stroke::new(1.0),
          );
        }

        self.draw_completions(cursor, render);
      }
    }
  }

  fn draw_status(&mut self, render: &mut Render) {
    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background_raised,
    );

    let line_height = render.store.text.font_metrics().line_height;

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
      let layout = render.layout_text(
        &format!("{}", self.editor.config.borrow().languages[&ft].display_name),
        render.theme().text,
      );
      render.draw_text(&layout, (render.size().width - 50.0, render.size().height - line_height));
    }
  }

  fn draw_progress(&mut self, render: &mut Render) {
    let line_height = render.store.text.font_metrics().line_height;

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

    for (i, p) in progress.iter().enumerate() {
      let layout = render.layout_text(p, render.theme().text);
      let y = progress.len() - i + 1;
      render.fill(
        &Rect::new(
          render.size().width - layout.size().width,
          render.size().height - line_height * y as f64,
          render.size().width,
          render.size().height - line_height * (y - 1) as f64,
        ),
        render.theme().background_raised,
      );
      render.draw_text(
        &layout,
        (render.size().width - layout.size().width, render.size().height - line_height * y as f64),
      );
    }
  }

  fn draw_trailing_spaces(&self, i: usize, end_of_line: f64, y: f64, render: &mut Render) {
    let mut shape = Circle::new((0.0, y + render.store.text.font_metrics().line_height / 2.0), 1.5);
    for (i, c) in self.doc().line(be_doc::Line(i)).chars().rev().enumerate() {
      if c == ' ' {
        shape.center.x = end_of_line
          - (i as f64 * render.store.text.font_metrics().character_width)
          - render.store.text.font_metrics().character_width / 2.0;
        render.fill(&shape, render.theme().background_raised);
      } else {
        break;
      }
    }
  }

  fn draw_change_gutter(&self, start_y: f64, render: &mut Render) {
    let line_height = render.store.text.font_metrics().line_height;

    if let Some(changes) = &self.editor.changes {
      for hunk in changes.hunks() {
        if be_doc::Line(hunk.after.end) < self.min_line
          || be_doc::Line(hunk.after.start) > self.max_line
        {
          continue;
        }

        for change in hunk.changes.iter().rev() {
          if be_doc::Line(change.after().end) < self.min_line
            || be_doc::Line(change.after().start) > self.max_line
          {
            continue;
          }

          if change.after().is_empty() {
            let y =
              start_y + (change.after().start - self.min_line.as_usize()) as f64 * line_height;

            let shape = Triangle::new((0.0, y - 4.0), (0.0, y + 4.0), (4.0, y));
            render.fill(&shape, render.theme().diff_remove);
          } else {
            let min_y = start_y
              + (change.after().start as f64 - self.min_line.as_usize() as f64) * line_height;
            let max_y =
              start_y + (change.after().end as f64 - self.min_line.as_usize() as f64) * line_height;

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
  }

  fn change_gutter_for_line(&self, line: be_doc::Line) -> Option<()> {
    self.editor.changes.as_ref()?.hunk_for_line(line).map(|_| ())
  }

  fn draw_completions(&mut self, cursor: Rect, render: &mut Render) {
    let line_height = render.store.text.font_metrics().line_height;
    let active = self.editor.active_completion();

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

      for (i, layout) in layouts.iter().enumerate() {
        if active == Some(i) {
          render.fill(
            &Rect::from_origin_size(Point::new(start_x, y), Size::new(inner_width, line_height)),
            render.theme().background_raised_outline,
          );
        }

        render.draw_text(&layout, (start_x, y));
        y += line_height;
      }
    }
  }

  fn layout_line(
    &mut self,
    layout: &mut Layout,
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
    let theme = &layout.store.theme;
    let mut text_layout =
      layout.store.text.layout_builder(&line_string, layout.theme().text, layout.scale());

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
          text_layout.color_range(range.clone(), foreground);
        }
        if let Some(weight) = highlight.weight {
          text_layout.apply(range.clone(), parley::StyleProperty::FontWeight(weight.to_parley()));
        }
        if let Some(underline) = highlight.underline {
          text_layout.apply(range.clone(), parley::StyleProperty::Underline(true));

          if let Underline::Color(c) = underline {
            text_layout.apply(range.clone(), parley::StyleProperty::UnderlineBrush(Some(c.into())));
          }
        }
        if let Some(background) = highlight.background {
          text_layout.background(range.clone(), background);
        }
      }

      if highlight.pos > max_index {
        break;
      }

      prev = pos;
    }

    let (text_layout, backgrounds) = text_layout.build(&line_string);
    let text_layout = layout.build_layout(text_layout, backgrounds);

    Some(entry.insert(text_layout))
  }

  fn cursor_mode(&self) -> Option<CursorMode> {
    if !self.focused() {
      return Some(CursorMode::Block);
    }

    match self.editor.mode() {
      Mode::Normal if self.temporary_underline => Some(CursorMode::Underline),
      Mode::Normal | Mode::Visual(_) => Some(CursorMode::Block),
      Mode::Insert => Some(CursorMode::Line),
      Mode::Replace => Some(CursorMode::Underline),
      Mode::Command => None,
    }
  }

  fn gutter_width(&self) -> f64 {
    self.line_number_width + LINE_NUMBER_MARGIN_LEFT + LINE_NUMBER_MARGIN_RIGHT
  }
}

struct IndentGuides {
  indent_width:  usize,
  scroll_offset: f64,
  margin:        f64,

  starts:       Vec<usize>,
  current_line: usize,
}

impl IndentGuides {
  pub fn new(indent_width: usize, scroll_offset: f64, margin: f64) -> Self {
    IndentGuides { indent_width, scroll_offset, margin, starts: vec![], current_line: 0 }
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
    const INDENT_GUIDE_START_OFFSET: f64 = 1.0;
    const INDENT_GUIDE_END_OFFSET: f64 = 1.0;

    let x = (self.starts.len() as f64
      * render.store.text.font_metrics().character_width
      * self.indent_width as f64)
      .round()
      + self.margin
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
