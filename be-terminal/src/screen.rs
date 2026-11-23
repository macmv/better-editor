//! A copy of the part of a terminal that is on screen.
//!
//! The emulator belongs to whoever reads the pty, so the ui never touches it.
//! The reader copies what changed into a `Screen` that is shared with the ui,
//! and the ui copies from that into its own `Screen`, and draws from that.
//! Both copies skip lines that are already up to date (see `OwnedLine::stamp`),
//! so they cost about as much as what changed.

use crate::{Cursor, Emulator, InputModes, MouseMode, MouseModes, OwnedLine, Size, TerminalState};

/// Lines of scrollback followed by the screen, which is what is drawn.
pub trait Lines {
  /// The line at `index` of scrollback followed by the screen, or `None` if it
  /// is out of range. Index 0 is the oldest line of scrollback.
  fn absolute_line(&self, index: usize) -> Option<&OwnedLine>;

  /// The number of lines of scrollback.
  fn scrollback_len(&self) -> usize;

  /// How far the viewport is scrolled back. 0 is live.
  fn viewport_offset(&self) -> usize;
}

impl Lines for TerminalState {
  fn absolute_line(&self, index: usize) -> Option<&OwnedLine> {
    TerminalState::absolute_line(self, index)
  }
  fn scrollback_len(&self) -> usize { TerminalState::scrollback_len(self) }
  fn viewport_offset(&self) -> usize { TerminalState::viewport_offset(self) }
}

/// The modes a program has set that the ui needs to know about, to encode input
/// and to decide what scrolling means.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Modes {
  pub input:      InputModes,
  pub mouse:      MouseModes,
  pub alt_screen: bool,
}

impl Modes {
  pub fn of(emu: &Emulator) -> Modes {
    Modes {
      input:      emu.input_modes(),
      mouse:      emu.mouse_modes(),
      alt_screen: emu.alt_screen(),
    }
  }
}

impl Default for Modes {
  fn default() -> Self {
    Modes {
      input:      InputModes::default(),
      mouse:      MouseModes {
        reporting: false,
        motion:    MouseMode::None,
        utf8:      false,
        focus:     false,
      },
      alt_screen: false,
    }
  }
}

/// The lines of a terminal that can be seen, and the cursor.
///
/// Lines keep their `id` and `stamp` when copied, so a renderer that caches by
/// those keeps working across snapshots.
pub struct Screen {
  // `lines[i]` is absolute line `first + i`.
  lines: Vec<OwnedLine>,
  first: usize,

  scrollback_len:  usize,
  viewport_offset: usize,
  size:            Size,

  pub cursor:       Cursor,
  pub(crate) modes: Modes,
}

impl Default for Screen {
  fn default() -> Self { Screen::new() }
}

impl Screen {
  /// An empty screen, until the first `Terminal::snapshot`.
  pub fn new() -> Self {
    Screen {
      lines:           vec![],
      first:           0,
      scrollback_len:  0,
      viewport_offset: 0,
      size:            Size { rows: 0, cols: 0 },
      cursor:          Cursor::default(),
      modes:           Modes::default(),
    }
  }

  /// The size of the terminal when this was copied.
  pub fn size(&self) -> Size { self.size }

  /// Returns a visible line, where 0 is the top of the viewport.
  pub fn visible_line(&self, index: usize) -> Option<&OwnedLine> {
    self.absolute_line((self.scrollback_len + index).checked_sub(self.viewport_offset)?)
  }

  /// Copies the visible lines of `state`, and one more row on each side for
  /// scrolling. Reuses the allocations of the previous snapshot.
  pub(crate) fn capture(&mut self, state: &TerminalState) {
    let scrollback_len = state.scrollback.len();
    let top = scrollback_len - state.viewport_offset.min(scrollback_len);
    let total = scrollback_len + state.size.rows;

    let first = top.saturating_sub(1);
    let end = (top + state.size.rows + 1).min(total);

    self.copy_lines((first..end).map_while(|index| state.absolute_line(index)));

    self.first = first;
    self.scrollback_len = scrollback_len;
    self.viewport_offset = state.viewport_offset;
    self.size = state.size;
    self.cursor = state.cursor;
  }

  /// Makes this a copy of `other`, which was made by `capture`. Lines that
  /// haven't changed since the last copy are left alone.
  pub(crate) fn capture_from(&mut self, other: &Screen) {
    self.copy_lines(other.lines.iter());

    self.first = other.first;
    self.scrollback_len = other.scrollback_len;
    self.viewport_offset = other.viewport_offset;
    self.size = other.size;
    self.cursor = other.cursor;
    self.modes = other.modes;
  }

  fn copy_lines<'a>(&mut self, lines: impl Iterator<Item = &'a OwnedLine>) {
    let mut count = 0;
    for (i, src) in lines.enumerate() {
      match self.lines.get_mut(i) {
        // The same id and stamp means the same cells, so there is nothing to
        // copy.
        Some(dst) if dst.id() == src.id() && dst.stamp() == src.stamp() => {}
        Some(dst) => dst.clone_from(src),
        None => self.lines.push(src.clone()),
      }
      count += 1;
    }
    self.lines.truncate(count);
  }
}

impl Lines for Screen {
  fn absolute_line(&self, index: usize) -> Option<&OwnedLine> {
    self.lines.get(index.checked_sub(self.first)?)
  }
  fn scrollback_len(&self) -> usize { self.scrollback_len }
  fn viewport_offset(&self) -> usize { self.viewport_offset }
}
