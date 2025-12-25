use be_input::{Action, Direction, Edit, Move};
use be_terminal::{StyleFlags, Terminal, TerminalColor};
use kurbo::Rect;
use parley::FontWeight;

use crate::{Color, Render, TextLayout, oklch, theme::Theme};

pub struct Shell {
  terminal:  Terminal,
  set_waker: bool,

  cached_layouts: Vec<TextLayout>,
  cached_scale:   f64,
}

impl Shell {
  pub fn new() -> Self {
    Shell {
      terminal:       Terminal::new(be_terminal::Size { rows: 40, cols: 80 }),
      set_waker:      false,
      cached_layouts: vec![],
      cached_scale:   0.0,
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
      Action::Edit { count: _, e: Edit::Delete } => self.terminal.perform_delete(),
      Action::Control { char: c @ 'a'..='z' } => self.terminal.perform_control(c as u8 - b'a' + 1),

      _ => {}
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    let line_height = render.store.text.font_metrics().line_height;
    let character_width = render.store.text.font_metrics().character_width;
    let height = (render.size().height / line_height).floor() as usize;
    let width = (render.size().width / character_width).floor() as usize;

    if !self.set_waker {
      self.set_waker = true;
      let waker = render.waker();
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

    self.terminal.set_size(be_terminal::Size { rows: height, cols: width });

    self.terminal.update();

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

    for line in 0..height {
      let Some(layout) = self.layout_line(render, line) else { break };

      render.draw_text(&layout, (0.0, line as f64 * line_height));
    }

    if self.terminal.state().cursor_visible {
      let cursor = self.terminal.state().cursor;
      render.fill(
        &Rect::from_origin_size(
          (
            (cursor.col as f64 * character_width).round(),
            (cursor.row as f64 * line_height).round(),
          ),
          (character_width.ceil(), line_height.ceil()),
        ),
        render.theme().text,
      );
    }
  }

  fn layout_line(&mut self, render: &mut Render, i: usize) -> Option<&mut TextLayout> {
    if self.cached_layouts.len() < i {
      return Some(&mut self.cached_layouts[i]);
    }

    let line = self.terminal.line(i)?;
    let line_string = line.to_string();

    let theme = &render.store.theme;
    let mut layout =
      render.store.text.layout_builder(&line_string, render.theme().text, render.scale());

    let mut prev = 0;
    for (style, i) in line.styles() {
      layout.color_range(prev..i, terminal_color(theme, style.foreground));
      if style.flags.contains(StyleFlags::BOLD) {
        layout.apply(prev..i, parley::StyleProperty::FontWeight(FontWeight::BLACK));
      }
      if style.flags.contains(StyleFlags::UNDERLINE) {
        layout.apply(prev..i, parley::StyleProperty::Underline(true));
      }
      prev = i;
    }

    let layout = layout.build(&line_string);
    let layout = render.build_layout(layout);

    if self.cached_layouts.len() == i {
      self.cached_layouts.push(layout);
    } else {
      self.cached_layouts[i] = layout;
    }

    Some(&mut self.cached_layouts[i])
  }
}

fn terminal_color(theme: &Theme, color: Option<TerminalColor>) -> Color {
  use be_terminal::BuiltinColor::*;

  match color {
    Some(TerminalColor::Builtin { color: Black, bright: _ }) => oklch(0.6, 0.0, 0.0),
    Some(TerminalColor::Builtin { color: Red, bright: _ }) => oklch(0.75, 0.13, 25.0),
    Some(TerminalColor::Builtin { color: Green, bright: _ }) => oklch(0.8, 0.14, 140.0),
    Some(TerminalColor::Builtin { color: Yellow, bright: _ }) => oklch(0.95, 0.12, 85.0),
    Some(TerminalColor::Builtin { color: Blue, bright: _ }) => oklch(0.8, 0.12, 240.0),
    Some(TerminalColor::Builtin { color: Magenta, bright: _ }) => oklch(0.8, 0.13, 350.0),
    Some(TerminalColor::Builtin { color: Cyan, bright: _ }) => oklch(0.85, 0.1, 200.0),
    Some(TerminalColor::Builtin { color: White, bright: _ }) => oklch(1.0, 0.0, 0.0),
    _ => theme.text,
  }
}
