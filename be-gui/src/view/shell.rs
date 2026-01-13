use be_input::{Action, Direction, Edit, Mode, Move};
use be_terminal::{StyleFlags, Terminal, TerminalColor};
use kurbo::Rect;
use parley::FontWeight;
use peniko::color::AlphaColor;

use crate::{Color, Render, TextLayout, oklch, theme::Theme};

pub struct Shell {
  terminal:  Terminal,
  set_waker: bool,

  cached_layouts: Vec<LineLayout>,
  cached_scale:   f64,
}

struct LineLayout {
  layout:     TextLayout,
  background: Vec<(f64, f64, Color)>,
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
      Action::Edit { count: _, e: Edit::Delete(_) } => self.terminal.perform_delete(),
      Action::Control { char: c @ 'a'..='z' } => self.terminal.perform_control(c as u8 - b'a' + 1),
      // Bit of a hack, but we're in "insert" mode, so escape sends us this.
      Action::SetMode { mode: Mode::Normal, .. } => self.terminal.perform_escape(),

      _ => {}
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    puffin::profile_function!();

    let line_height = render.store.text.font_metrics().line_height;
    let character_width = render.store.text.font_metrics().character_width;
    let height = (render.size().height / line_height).floor() as usize;
    let width = (render.size().width / character_width).floor() as usize;

    if !self.set_waker {
      self.set_waker = true;
      let waker = render.notifier();
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

      for range in &layout.background {
        render.fill(
          &Rect::from_origin_size(
            (range.0.round(), (line as f64 * line_height).round()),
            ((range.1 - range.0).ceil(), line_height.ceil()),
          ),
          range.2,
        );
      }
      render.draw_text(&layout.layout, (0.0, line as f64 * line_height));
    }

    if self.terminal.state().cursor.visible {
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
    for (style, i) in line.styles() {
      if let Some(color) = terminal_color(theme, style.foreground) {
        layout.color_range(prev..i, color);
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
    for (b, i) in line.specific_styles(|s| s.background) {
      let x = layout.cursor(i, crate::CursorMode::Line).x0;
      if let Some(color) = terminal_color(&render.store.theme, b) {
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
