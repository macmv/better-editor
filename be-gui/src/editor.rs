use std::collections::HashMap;

use be_doc::crop::RopeSlice;
use be_editor::EditorState;
use be_input::{Action, Key, KeyStroke, Mode};
use kurbo::{Axis, Line, Point, Rect, Stroke, Vec2};

use crate::{CursorMode, Distance, Render, TextLayout, file_tree::FileTree};

pub struct Editor {
  root: Pane,
}

enum Pane {
  Content(Content),
  Split(Split),
}

enum Content {
  Editor(EditorView),
  FileTree(FileTree),
}

struct EditorView {
  editor: EditorState,

  scroll:  Point,
  focused: bool,

  cached_layouts: HashMap<usize, TextLayout>,
  cached_scale:   f64,
}

struct Split {
  axis:    Axis,
  percent: f64,
  active:  Side,
  left:    Box<Pane>,
  right:   Box<Pane>,
}

#[derive(Copy, Clone)]
enum Side {
  Left,
  Right,
}

impl Pane {
  fn draw(&mut self, render: &mut Render) {
    match self {
      Pane::Content(content) => content.draw(render),
      Pane::Split(split) => split.draw(render),
    }
  }

  fn active(&self) -> &Content {
    match self {
      Pane::Content(content) => content,
      Pane::Split(split) => split.active(),
    }
  }

  fn active_mut(&mut self) -> &mut Content {
    match self {
      Pane::Content(content) => content,
      Pane::Split(split) => split.active_mut(),
    }
  }

  fn focus(&mut self, direction: Direction) -> bool {
    match self {
      Pane::Content(_) => false,
      Pane::Split(split) => split.focus(direction),
    }
  }
}

impl Split {
  fn draw(&mut self, render: &mut Render) {
    render.split(
      self,
      self.axis,
      Distance::Percent(self.percent),
      |state, render| state.left.draw(render),
      |state, render| state.right.draw(render),
    );
  }

  fn active(&self) -> &Content {
    match self.active {
      Side::Left => self.left.active(),
      Side::Right => self.right.active(),
    }
  }

  fn active_mut(&mut self) -> &mut Content {
    match self.active {
      Side::Left => self.left.active_mut(),
      Side::Right => self.right.active_mut(),
    }
  }

  /// Returns true if the focus changed.
  fn focus(&mut self, direction: Direction) -> bool {
    let focused = match self.active {
      Side::Left => &mut self.left,
      Side::Right => &mut self.right,
    };

    if !focused.focus(direction) {
      match (self.active, self.axis, direction) {
        (Side::Left, Axis::Vertical, Direction::Right) => self.active = Side::Right,
        (Side::Right, Axis::Vertical, Direction::Left) => self.active = Side::Left,
        (Side::Left, Axis::Horizontal, Direction::Down) => self.active = Side::Right,
        (Side::Right, Axis::Horizontal, Direction::Up) => self.active = Side::Left,

        _ => return false,
      }

      match self.active {
        Side::Left => {
          self.left.active_mut().on_focus(true);
          self.right.active_mut().on_focus(false);
        }
        Side::Right => {
          self.right.active_mut().on_focus(true);
          self.left.active_mut().on_focus(false);
        }
      }

      true
    } else {
      false
    }
  }
}

impl Content {
  fn draw(&mut self, render: &mut Render) {
    match self {
      Content::Editor(editor) => editor.draw(render),
      Content::FileTree(file_tree) => file_tree.draw(render),
    }
  }

  fn mode(&self) -> Mode {
    match self {
      Content::Editor(editor) => editor.editor.mode(),
      Content::FileTree(_) => Mode::Normal,
    }
  }

  fn perform_action(&mut self, action: Action) {
    match self {
      Content::Editor(editor) => editor.editor.perform_action(action),
      Content::FileTree(file_tree) => file_tree.perform_action(action),
    }
  }

  fn on_focus(&mut self, focus: bool) {
    match self {
      Content::Editor(editor) => editor.on_focus(focus),
      Content::FileTree(file_tree) => file_tree.on_focus(focus),
    }
  }
}

impl Editor {
  pub fn new() -> Self {
    Editor {
      root: Pane::Split(Split {
        axis:    Axis::Vertical,
        percent: 0.2,
        active:  Side::Right,
        left:    Box::new(Pane::Content(Content::FileTree(FileTree::current_directory()))),
        right:   Box::new(Pane::Content(Content::Editor(EditorView {
          editor:         EditorState::from("💖hello\n💖foobar\nsdjkhfl\nworld\n"),
          scroll:         Point::ZERO,
          focused:        true,
          cached_layouts: HashMap::new(),
          cached_scale:   0.0,
        }))),
      }),
    }
  }

  pub fn open(&mut self, path: &std::path::Path) {
    match self.root.active_mut() {
      Content::Editor(editor) => {
        let _ = editor.editor.open(path);
      }
      Content::FileTree(_) => {}
    }
  }

  pub fn on_key(&mut self, keys: &[KeyStroke]) -> Result<(), be_input::ActionError> {
    if keys.get(0).is_some_and(|k| k.control && k.key == 'w') {
      if keys.len() == 1 {
        return Err(be_input::ActionError::Incomplete);
      }

      match keys[1].key {
        Key::Char('h') => {
          self.root.focus(Direction::Left);
        }
        Key::Char('j') => {
          self.root.focus(Direction::Down);
        }
        Key::Char('k') => {
          self.root.focus(Direction::Up);
        }
        Key::Char('l') => {
          self.root.focus(Direction::Right);
        }
        _ => {}
      }

      return Ok(());
    }

    let action = Action::from_input(self.root.active().mode(), keys)?;
    self.root.active_mut().perform_action(action);

    Ok(())
  }

  pub fn draw(&mut self, render: &mut Render) { self.root.draw(render); }
}

#[derive(Copy, Clone)]
enum Direction {
  Up,
  Down,
  Left,
  Right,
}

impl EditorView {
  fn on_focus(&mut self, focus: bool) { self.focused = focus; }

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

    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background,
    );

    let line_height = render.store.text.font_metrics().line_height;

    const SCROLL_OFF: usize = 5;

    let min_fully_visible_row = (self.scroll.y / line_height).ceil() as usize + SCROLL_OFF;
    let max_fully_visible_row =
      ((self.scroll.y + render.size().height) / line_height).floor() as usize - 1 - SCROLL_OFF;

    if self.editor.cursor().line.as_usize() < min_fully_visible_row {
      let target_line = self
        .editor
        .cursor()
        .line
        .as_usize()
        .saturating_sub(SCROLL_OFF)
        .clamp(0, self.editor.doc().rope.lines().len());

      self.scroll.y = target_line as f64 * line_height;
    } else if self.editor.cursor().line.as_usize() > max_fully_visible_row {
      let target_line = self
        .editor
        .cursor()
        .line
        .as_usize()
        .saturating_add(SCROLL_OFF + 1)
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

    let mut y = -(self.scroll.y % line_height);
    let mut indent_guides = IndentGuides::new(y);
    loop {
      if self.layout_line(render, i, index).is_none() {
        break;
      };
      indent_guides
        .visit(self.editor.doc().rope.byte_slice(index..).raw_lines().next().unwrap(), render);

      let layout = self.cached_layouts.get(&i).unwrap();

      if self.focused && self.editor.cursor().line == i {
        let mode = match self.editor.mode() {
          Mode::Normal | Mode::Visual => Some(CursorMode::Block),
          Mode::Insert => Some(CursorMode::Line),
          Mode::Replace => Some(CursorMode::Underline),
          Mode::Command => None,
        };

        if let Some(mode) = mode {
          let cursor = layout.cursor(self.editor.cursor_column_byte(), mode) + Vec2::new(20.0, y);
          render.fill(&cursor, render.theme().text);
        }
      }

      render.draw_text(&layout, Point::new(20.0, y));

      y += line_height;
      i += 1;
      index += self.editor.doc().rope.byte_slice(index..).raw_lines().next().unwrap().byte_len();
      if index >= end {
        break;
      }
    }

    indent_guides.finish(render);

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

      let layout = render.layout_text(&command.text, render.theme().text);
      render.draw_text(&layout, (20.0, render.size().height - line_height));

      let cursor = layout.cursor(command.cursor as usize, CursorMode::Line);
      render.fill(&cursor, render.theme().text);
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

      if let Some(color) = theme.syntax.lookup(&highlight.highlights) {
        layout.color_range(prev - index..pos - index, color);
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
  pub fn new(scroll_offset: f64) -> Self {
    const INDENT_WIDTH: usize = 2; // TODO
    IndentGuides { indent_width: INDENT_WIDTH, scroll_offset, starts: vec![], current_line: 0 }
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
