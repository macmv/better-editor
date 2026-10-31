use std::path::{Path, PathBuf};

use be_input::{Action, Clipboard, Direction, Edit, Mode, Move};
use be_shared::SharedHandle;
use be_terminal::{Position, Terminal};
use kurbo::{Point, Rect, Size};

use super::terminal_paint::{CursorDraw, DrawOptions, TerminalPainter, character_size, grid_size};
use crate::{Layout, MouseButton, MouseEvent, Render, RenderStore};

pub struct TerminalView {
  terminal:  SharedHandle<Terminal>,
  set_waker: bool,
  focused:   bool,

  // Scroll offset. Note that this is inverted compared to the editor. Terminals scroll backwards
  // as much as they want, but always have a fixed bottom. So 0 means no scroll, and 1 means shift
  // everything _down_ 1 line (which would be offset=-1 in the editor).
  viewport_offset: f64, // 0..1, partial line offset

  size:           be_terminal::Size,
  character_size: kurbo::Size,

  painter: TerminalPainter,

  clipboard: SharedHandle<Clipboard>,

  drag_start: Option<Point>,
  selection:  Option<(Position, Position)>,
  mouse_held: Option<u8>,
  mouse_pos:  Option<Point>,
}

impl TerminalView {
  pub fn new(clipboard: &SharedHandle<Clipboard>, path: &Path) -> Self {
    let terminal = Terminal::new(be_terminal::Size { rows: 40, cols: 80 }, path);
    TerminalView::from_handle(clipboard, SharedHandle::new(terminal))
  }

  pub fn from_handle(
    clipboard: &SharedHandle<Clipboard>,
    terminal: SharedHandle<Terminal>,
  ) -> Self {
    TerminalView {
      terminal,
      set_waker: false,
      focused: false,
      viewport_offset: 0.0,
      size: be_terminal::Size { rows: 40, cols: 80 },
      character_size: kurbo::Size::ZERO,
      painter: TerminalPainter::new(),
      clipboard: clipboard.clone(),
      drag_start: None,
      selection: None,
      mouse_held: None,
      mouse_pos: None,
    }
  }

  pub fn current_dir(&self) -> PathBuf { self.terminal.current_dir() }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { count: _, m: Move::Single(Direction::Up) } => self.terminal.perform_up(),
      Action::Move { count: _, m: Move::Single(Direction::Down) } => self.terminal.perform_down(),
      Action::Move { count: _, m: Move::Single(Direction::Left) } => self.terminal.perform_left(),
      Action::Move { count: _, m: Move::Single(Direction::Right) } => self.terminal.perform_right(),
      Action::Edit { count: _, e: Edit::Insert('\n') } => self.terminal.perform_input('\r'),
      Action::Edit { count: _, e: Edit::Insert(c) } => self.terminal.perform_input(c),
      Action::Edit { count: _, e: Edit::Backspace } => self.terminal.perform_backspace(),
      Action::Edit { count: _, e: Edit::Delete(_) } => self.terminal.perform_delete(),
      Action::Control { char: '\n' } => self.terminal.perform_ctrl_enter(),
      Action::Control { char: c @ 'a'..='z' } => self.terminal.perform_control(c as u8 - b'a' + 1),
      // Bit of a hack, but we're in "insert" mode, so escape sends us this.
      Action::SetMode { mode: Mode::Normal, .. } => self.terminal.perform_escape(),
      Action::Tab { shift: false } => self.terminal.perform_tab(),
      Action::Tab { shift: true } => self.terminal.perform_shift_tab(),

      Action::ClipboardCopy => {
        self.copy();
        return; // Don't clear the selection.
      }
      Action::ClipboardPaste => self.terminal.perform_paste(&self.clipboard.paste()),

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
    match *ev {
      MouseEvent::Move { pos } => {
        self.mouse_pos = Some(pos);
        let mouse = self.mouse_to_pos(pos);

        self.terminal.perform_mouse_move(mouse.col, mouse.row, self.mouse_held);

        if let Some(start) = self.drag_start {
          let anchor = self.mouse_to_pos(start);

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

      MouseEvent::Button { pos, pressed, button } => {
        let mouse = self.mouse_to_pos(pos);
        let btn_code = match button {
          MouseButton::Left => 0,
          MouseButton::Middle => 1,
          MouseButton::Right => 2,
        };

        self.terminal.perform_mouse_button(mouse.col, mouse.row, btn_code, pressed);

        if pressed {
          self.mouse_held = Some(btn_code);
        } else {
          self.mouse_held = None;
        }

        if button == MouseButton::Left {
          if pressed {
            self.drag_start = Some(pos);
            self.selection = None;
          } else {
            self.drag_start = None;
          }
        }
      }

      MouseEvent::Scroll { pos, delta } => {
        let cell = self.mouse_to_pos(pos);
        if self.terminal.is_fractional_scrolling() {
          // TODO: Constant for line_height = 20.0
          self.viewport_offset += delta.y / 20.0;
          let terminal_offset = self.terminal.viewport_offset();
          self.viewport_offset = self.viewport_offset.clamp(
            -(terminal_offset as isize) as f64,
            (self.terminal.maximum_offset() - terminal_offset) as f64,
          );

          let lines = self.viewport_offset.floor() as isize;
          self.viewport_offset -= lines as f64;
          self.terminal.perform_mouse_scroll(cell.col, cell.row, lines as i32);
        } else {
          self.terminal.perform_mouse_scroll(cell.col, cell.row, (delta.y / 20.0) as i32);
        }
      }

      MouseEvent::Leave => {
        self.drag_start = None;
        self.mouse_held = None;
        self.mouse_pos = None;
      }

      _ => {}
    }

    crate::CursorKind::Beam
  }

  pub fn layout(&mut self, layout: &mut Layout) {
    puffin::profile_function!();

    if self.terminal.update() {
      layout.close_view();
    }

    self.character_size = character_size(layout.store);
    self.size = grid_size(layout.size(), self.character_size);

    if !self.set_waker {
      self.set_waker = true;
      let waker = layout.notifier();
      // The host throttles how often this is called.
      self.terminal.set_wake(Box::new(move || waker.wake()));
    }

    self.terminal.set_size(self.size);

    // There is no wakeup for output we left in the pty to keep this frame
    // short.
    if self.terminal.has_pending_writes() || self.terminal.has_pending_output() {
      layout.notifier().wake();
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    puffin::profile_function!();

    let size = render.size();
    render.fill(&Rect::new(0.0, 0.0, size.width, size.height), render.theme().background);

    let state = self.terminal.state();
    let mut options =
      DrawOptions::screen(state, Rect::from_origin_size((0.0, 0.0), size), self.size.rows);
    options.pixel_offset = self.viewport_offset;
    options.selection = self.selection;
    options.cursor =
      state.cursor.visible.then(|| CursorDraw { pos: state.cursor.pos, focused: self.focused });

    self.painter.draw(render, state, &options);
  }

  pub fn on_focus(&mut self, focus: bool) {
    self.focused = focus;
    self.terminal.perform_focus_event(focus);
  }

  fn mouse_to_pos(&self, mouse: Point) -> Position {
    Position {
      row: (mouse.y / self.character_size.height) as usize,
      col: (mouse.x / self.character_size.width).round() as usize,
    }
  }

  fn copy(&self) {
    let Some((start, end)) = self.selection else { return };
    let mut buf = String::new();

    for row in start.row..=end.row {
      let Some(line) = self.terminal.visible_line(row) else { continue };
      let slice = if row == start.row && row == end.row {
        line.slice_to_string(start.col..end.col)
      } else if row == start.row {
        line.slice_to_string(start.col..)
      } else if row == end.row {
        line.slice_to_string(..end.col)
      } else {
        line.slice_to_string(..)
      };

      if !buf.is_empty() {
        buf.push('\n');
      }
      buf.push_str(slice.trim_end());
    }

    self.clipboard.copy(&buf);
  }
}
