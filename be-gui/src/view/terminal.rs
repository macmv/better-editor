use be_input::{Action, Clipboard, Direction, Edit, Mode, Move};
use be_shared::SharedHandle;
use be_terminal::{Position, StyleFlags, Terminal, TerminalColor};
use be_workspace::Workspace;
use kurbo::{Point, Rect, Size, Stroke};
use parley::FontWeight;
use peniko::color::AlphaColor;

use crate::{
  Color, Layout, MouseButton, MouseEvent, Render, RenderStore, TextLayout, oklch, theme::Theme,
};

pub struct TerminalView {
  terminal:  Terminal,
  set_waker: bool,
  focused:   bool,

  size:           be_terminal::Size,
  character_size: kurbo::Size,

  cached_layouts: Vec<LineLayout>,
  cached_scale:   f64,

  clipboard: SharedHandle<Clipboard>,

  drag_start: Option<Point>,
  selection:  Option<(Position, Position)>,
}

struct LineLayout {
  layout:     TextLayout,
  background: Vec<(f64, f64, Color)>,
}

impl TerminalView {
  pub fn new(workspace: &Workspace) -> Self {
    TerminalView {
      terminal:       Terminal::new(be_terminal::Size { rows: 40, cols: 80 }),
      set_waker:      false,
      focused:        false,
      size:           be_terminal::Size { rows: 40, cols: 80 },
      character_size: kurbo::Size::ZERO,
      cached_layouts: vec![],
      cached_scale:   0.0,
      clipboard:      workspace.clipboard.clone(),
      drag_start:     None,
      selection:      None,
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { count: _, m: Move::Single(Direction::Up) } => self.terminal.perform_up(),
      Action::Move { count: _, m: Move::Single(Direction::Down) } => self.terminal.perform_down(),
      Action::Move { count: _, m: Move::Single(Direction::Left) } => self.terminal.perform_left(),
      Action::Move { count: _, m: Move::Single(Direction::Right) } => self.terminal.perform_right(),
      Action::Edit { count: _, e: Edit::Insert(c) } => self.terminal.perform_input(c),
      Action::Edit { count: _, e: Edit::Backspace } => self.terminal.perform_backspace(),
      Action::Edit { count: _, e: Edit::Delete(_) } => self.terminal.perform_delete(),
      Action::Control { char: c @ 'a'..='z' } => self.terminal.perform_control(c as u8 - b'a' + 1),
      // Bit of a hack, but we're in "insert" mode, so escape sends us this.
      Action::SetMode { mode: Mode::Normal, .. } => self.terminal.perform_escape(),
      Action::Tab => self.terminal.perform_tab(),

      Action::Paste => self.terminal.perform_paste(&self.clipboard.paste()),

      _ => {}
    }

    self.selection = None;
  }

  pub fn on_mouse(
    &mut self,
    ev: &crate::MouseEvent,
    _size: Size,
    _store: &RenderStore,
  ) -> crate::CursorKind {
    match ev {
      MouseEvent::Move { pos } => {
        if let Some(start) = self.drag_start {
          let anchor = self.mouse_to_pos(start);
          let mouse = self.mouse_to_pos(*pos);

          if anchor != mouse {
            let (min, max) = if anchor.row == mouse.row {
              let min_col = anchor.col.min(mouse.col);
              let max_col = anchor.col.max(mouse.col);
              (
                Position { row: anchor.row, col: min_col },
                Position { row: anchor.row, col: max_col },
              )
            } else if anchor.row < mouse.row {
              (anchor, mouse)
            } else {
              (mouse, anchor)
            };

            self.selection = Some((min, max));
          } else {
            self.selection = None;
          }
        }
      }

      MouseEvent::Button { pos, pressed: true, button: MouseButton::Left } => {
        self.drag_start = Some(*pos);
        self.selection = None;
      }

      MouseEvent::Button { pos: _, pressed: false, button: MouseButton::Left } => {
        self.drag_start = None;
      }
      MouseEvent::Leave => self.drag_start = None,

      _ => {}
    }

    crate::CursorKind::Beam
  }

  pub fn layout(&mut self, layout: &mut Layout) {
    puffin::profile_function!();

    if self.terminal.update() {
      layout.close_view();
    }

    self.character_size.height = layout.store.text.font_metrics().line_height;
    self.character_size.width = layout.store.text.font_metrics().character_width;
    self.size.rows = (layout.size().height / self.character_size.height).floor() as usize;
    self.size.cols = (layout.size().width / self.character_size.width).floor() as usize;

    if !self.set_waker {
      self.set_waker = true;
      let waker = layout.notifier();
      // SAFETY: This isn't safe. Need to join the thread on drop.
      let poller = unsafe { self.terminal.make_poller() };
      std::thread::spawn(move || {
        loop {
          poller.poll();
          waker.wake();

          // We only need to wake it up once per frame, so don't spam wake ups.
          std::thread::sleep(std::time::Duration::from_millis(10));
        }
      });
    }

    self.terminal.set_size(self.size);
    self.terminal.update();
  }

  pub fn draw(&mut self, render: &mut Render) {
    puffin::profile_function!();

    if self.cached_scale != render.scale() {
      self.cached_layouts.clear();
      self.cached_scale = render.scale();
    }

    // TODO
    /*
    if self.editor.take_damage_all() {
      self.cached_layouts.clear();
    }

    for line in self.editor.take_damages() {
      self.cached_layouts.remove(&line.as_usize());
    }
    */

    render.fill(
      &Rect::new(0.0, 0.0, render.size().width, render.size().height),
      render.theme().background,
    );

    let character_size = self.character_size;

    for line in 0..self.size.rows {
      let Some(layout) = self.layout_line(render, line) else { break };

      for range in &layout.background {
        render.fill(
          &Rect::from_origin_size(
            (range.0.round(), (line as f64 * character_size.height).round()),
            ((range.1 - range.0).ceil(), character_size.height.ceil()),
          ),
          range.2,
        );
      }
      render.draw_text(&layout.layout, (0.0, line as f64 * character_size.height));
    }

    if self.terminal.state().cursor.visible {
      let cursor = self.terminal.state().cursor;
      let cursor = Rect::from_origin_size(
        (
          (cursor.col as f64 * self.character_size.width).round(),
          (cursor.row as f64 * self.character_size.height).round(),
        ),
        (self.character_size.width.ceil(), self.character_size.height.ceil()),
      );
      if self.focused {
        render.fill(&cursor.ceil(), render.theme().text);
      } else {
        render.stroke(&cursor.inset(-0.5 * render.scale()), render.theme().text, Stroke::new(1.0));
      }
    }
  }

  pub fn on_focus(&mut self, focus: bool) { self.focused = focus; }

  fn layout_line(&mut self, render: &mut Render, i: usize) -> Option<&mut LineLayout> {
    if self.cached_layouts.len() < i {
      return Some(&mut self.cached_layouts[i]);
    }

    let line = self.terminal.line(i)?;
    let line_string = line.to_string();

    let theme = &render.store.theme;
    let mut layout =
      render.store.text.layout_builder(&line_string, render.theme().text, render.scale());

    let mut prev = 0;
    for ((foreground, style), i) in line.specific_styles(|j, s| {
      let foreground = if self.inverted_at(i, j) {
        Some(terminal_color(theme, s.background).unwrap_or(theme.background))
      } else {
        terminal_color(theme, s.foreground)
      };
      (foreground, s)
    }) {
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
    let layout = render.build_layout(layout, backgrounds);

    let mut background = vec![];
    let mut prev = 0.0;
    for (color, i) in line.specific_styles(|j, s| {
      if self.inverted_at(i, j) {
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

    let layout = LineLayout { layout, background };

    if self.cached_layouts.len() == i {
      self.cached_layouts.push(layout);
    } else {
      self.cached_layouts[i] = layout;
    }

    Some(&mut self.cached_layouts[i])
  }

  fn mouse_to_pos(&self, mouse: Point) -> Position {
    Position {
      row: (mouse.y / self.character_size.height) as usize,
      col: (mouse.x / self.character_size.width).round() as usize,
    }
  }

  fn inverted_at(&self, row: usize, col: usize) -> bool {
    if let Some((start, end)) = self.selection {
      if row < start.row || row > end.row {
        false
      } else if row == start.row && row == end.row {
        col >= start.col && col < end.col
      } else if row == start.row {
        col >= start.col
      } else if row == end.row {
        col < end.col
      } else {
        true
      }
    } else {
      false
    }
  }
}

fn terminal_color(_theme: &Theme, color: Option<TerminalColor>) -> Option<Color> {
  use be_terminal::BuiltinColor::*;

  Some(match color {
    Some(TerminalColor::Builtin { color: Black, bright: _ }) => oklch(0.6, 0.0, 0.0),
    Some(TerminalColor::Builtin { color: Red, bright: _ }) => oklch(0.75, 0.13, 25.0),
    Some(TerminalColor::Builtin { color: Green, bright: _ }) => oklch(0.8, 0.14, 140.0),
    Some(TerminalColor::Builtin { color: Yellow, bright: _ }) => oklch(0.95, 0.12, 85.0),
    Some(TerminalColor::Builtin { color: Blue, bright: _ }) => oklch(0.8, 0.12, 240.0),
    Some(TerminalColor::Builtin { color: Magenta, bright: _ }) => oklch(0.8, 0.13, 350.0),
    Some(TerminalColor::Builtin { color: Cyan, bright: _ }) => oklch(0.85, 0.1, 200.0),
    Some(TerminalColor::Builtin { color: White, bright: _ }) => oklch(1.0, 0.0, 0.0),
    Some(TerminalColor::Rgb { r, g, b }) => AlphaColor::from_rgb8(r, g, b).convert(),
    _ => return None,
  })
}
