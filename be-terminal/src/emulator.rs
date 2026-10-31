use std::time::Instant;

use anstyle_parse::{Parser, Utf8Parser};

use crate::{
  InputModes, KITTY_DISAMBIGUATE, SYNC_END, SYNC_MAX_HELD, SYNC_TIMEOUT, Size, TerminalFlags,
  TerminalState, advance,
};

/// How a program wants mouse events reported to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseMode {
  /// The program doesn't want mouse events.
  None,
  /// Button presses and releases (mode 1000).
  Press,
  /// Presses, releases, and motion while a button is held (mode 1002).
  Drag,
  /// All motion (mode 1003).
  Motion,
}

/// A terminal emulator with no pty: bytes go in, a screen comes out. Replies
/// the program is waiting for (cursor position reports, device attributes, and
/// so on) are collected for the caller to send back.
pub struct Emulator {
  pub(crate) state: TerminalState,
  parser:           Parser,

  // Output received during a synchronized update, which isn't parsed until the update ends, so
  // that the screen never shows half of a frame. Held until `hold_deadline`, if it is set.
  held:                     Vec<u8>,
  held_scanned:             usize,
  pub(crate) hold_deadline: Option<Instant>,
}

impl Emulator {
  pub fn new(size: Size) -> Self {
    Emulator {
      state:         TerminalState::new(size),
      parser:        Parser::<Utf8Parser>::new(),
      held:          Vec::new(),
      held_scanned:  0,
      hold_deadline: None,
    }
  }

  pub fn state(&self) -> &TerminalState { &self.state }

  /// Parses output from the program. Output after the start of a synchronized
  /// update is held until it ends (or times out, see `tick`).
  pub fn feed(&mut self, bytes: &[u8]) {
    self.state.begin_frame();
    self.consume(bytes);
    self.release_held();
  }

  /// Releases held output if its synchronized update has timed out. Callers
  /// should call this soon if `is_holding` is true.
  pub fn tick(&mut self) {
    self.state.begin_frame();
    self.release_held();
  }

  /// Parses any held output no matter what, for when the program has exited.
  pub fn finish(&mut self) {
    self.state.begin_frame();
    self.finish_hold();
  }

  /// Whether output is being held for a synchronized update.
  pub fn is_holding(&self) -> bool { self.hold_deadline.is_some() }

  /// Takes the bytes that should be sent back to the program.
  pub fn take_replies(&mut self) -> Vec<u8> { std::mem::take(&mut self.state.pending_writes) }

  pub fn resize(&mut self, size: Size) {
    if size != self.state.size {
      self.state.begin_frame();
      self.state.resize(size);
    }
  }

  pub fn size(&self) -> Size { self.state.size }

  /// The number of lines with content: all of scrollback, plus the screen up
  /// to the last row that has text or the cursor. On the alternate screen, this
  /// is the whole screen.
  pub fn used_rows(&self) -> usize {
    let scrollback = self.state.scrollback.len();
    if self.state.alt_screen {
      return scrollback + self.state.size.rows;
    }

    let mut used = self.state.cursor.row + 1;
    for row in (used..self.state.size.rows).rev() {
      if let Some(line) = self.state.grid.line(row)
        && !line.to_string().trim_end().is_empty()
      {
        used = row + 1;
        break;
      }
    }

    scrollback + used.min(self.state.size.rows)
  }

  /// A row of the visible screen.
  pub fn screen_line(&self, row: usize) -> Option<&crate::OwnedLine> {
    // The grids are swapped when entering the alternate screen, so `grid` is
    // always visible.
    self.state.grid.line(row)
  }

  /// Scrollback and screen as text, one string per line, with trailing blanks
  /// removed. Trailing empty lines on the screen are dropped.
  pub fn lines(&self) -> Vec<String> {
    let mut out: Vec<String> =
      self.state.scrollback.iter().map(|l| l.to_string().trim_end().to_string()).collect();
    for row in 0..self.state.size.rows {
      if let Some(l) = self.screen_line(row) {
        out.push(l.to_string().trim_end().to_string());
      }
    }
    while out.last().is_some_and(|l| l.is_empty()) {
      out.pop();
    }
    out
  }

  /// [`Emulator::lines`] joined with newlines.
  pub fn text(&self) -> String { self.lines().join("\n") }

  pub fn alt_screen(&self) -> bool { self.state.alt_screen }

  pub fn mouse_mode(&self) -> MouseMode {
    let f = &self.state.flags;
    if f.contains(TerminalFlags::MOUSE_ALL_MOTION) {
      MouseMode::Motion
    } else if f.contains(TerminalFlags::MOUSE_MOTION) {
      MouseMode::Drag
    } else if f.contains(TerminalFlags::REPORT_MOUSE) {
      MouseMode::Press
    } else {
      MouseMode::None
    }
  }

  /// Whether the program wants mouse events at all.
  pub fn mouse_reporting(&self) -> bool { self.state.flags.contains(TerminalFlags::REPORT_MOUSE) }
  /// Whether mouse coordinates use the UTF-8 encoding (mode 1005).
  pub fn mouse_utf8(&self) -> bool { self.state.flags.contains(TerminalFlags::MOUSE_UTF8) }
  /// Whether the program wants focus events (mode 1004).
  pub fn report_focus(&self) -> bool { self.state.flags.contains(TerminalFlags::REPORT_FOCUS) }
  pub fn bracketed_paste(&self) -> bool {
    self.state.flags.contains(TerminalFlags::BRACKETED_PASTE)
  }
  /// Whether arrow keys send `ESC O A` instead of `ESC [ A` (DECCKM).
  pub fn application_cursor_keys(&self) -> bool {
    self.state.flags.contains(TerminalFlags::CURSOR_KEYS)
  }
  pub fn keypad_application_mode(&self) -> bool {
    self.state.flags.contains(TerminalFlags::KEYPAD_APPLICATION_MODE)
  }
  /// The active kitty keyboard protocol flags.
  pub fn kitty_flags(&self) -> u8 { self.state.kitty_flags }
  /// Whether Ctrl+Enter and friends should use `CSI u` encodings.
  pub fn kitty_disambiguate(&self) -> bool { self.state.kitty_flags & KITTY_DISAMBIGUATE != 0 }

  /// The modes that change which mouse and focus events the program gets.
  pub fn mouse_modes(&self) -> crate::MouseModes {
    crate::MouseModes {
      reporting: self.mouse_reporting(),
      motion:    self.mouse_mode(),
      utf8:      self.mouse_utf8(),
      focus:     self.report_focus(),
    }
  }

  /// The modes that change how keys are encoded for this program.
  pub fn input_modes(&self) -> InputModes {
    InputModes {
      application_cursor: self.application_cursor_keys(),
      kitty_flags:        self.kitty_flags(),
      bracketed_paste:    self.bracketed_paste(),
    }
  }

  /// Parses `bytes`, unless a synchronized update is in progress, in which case
  /// they are held until it ends.
  pub(crate) fn consume(&mut self, mut bytes: &[u8]) {
    if self.hold_deadline.is_some() {
      self.held.extend_from_slice(bytes);
      return;
    }

    while !bytes.is_empty() {
      let used = advance(&mut self.parser, &mut self.state, bytes);
      bytes = &bytes[used..];

      if std::mem::take(&mut self.state.sync_started) {
        self.held.clear();
        self.held.extend_from_slice(bytes);
        self.held_scanned = 0;
        self.hold_deadline = Some(Instant::now() + SYNC_TIMEOUT);
        return;
      }
    }
  }

  /// Whether a synchronized update has ended, or has gone on for too long.
  fn should_release(&mut self) -> bool {
    let Some(deadline) = self.hold_deadline else { return false };

    // Only look at the new bytes, but the sequence might be split across reads.
    let from = self.held_scanned.saturating_sub(SYNC_END.len() - 1);
    self.held_scanned = self.held.len();

    Instant::now() >= deadline
      || self.held.len() > SYNC_MAX_HELD
      || self.held[from..].windows(SYNC_END.len()).any(|w| w == SYNC_END)
  }

  /// Parses held output if its synchronized update is over. Output after the
  /// end of the update can start another one, which holds the rest of it
  /// again.
  pub(crate) fn release_held(&mut self) {
    while self.should_release() {
      self.hold_deadline = None;

      let held = std::mem::take(&mut self.held);
      self.consume(&held);

      // This is already off if the update ended, but not if it timed out.
      if self.hold_deadline.is_none() {
        self.state.synchronized = false;
      }
    }
  }

  /// Parses held output no matter what, for when the pty is closing.
  pub(crate) fn finish_hold(&mut self) {
    if self.hold_deadline.is_some() {
      self.hold_deadline = Some(Instant::now());
      self.release_held();
    }
  }
}
