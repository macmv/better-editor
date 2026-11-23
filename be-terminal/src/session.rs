//! How a terminal's output gets from the pty to the ui.
//!
//! A `Core` owns the emulator, and nothing else touches it. After it parses
//! output, it publishes what changed by copying it into a `Screen` in
//! `Shared`. The ui copies that into its own `Screen` when it is about to draw,
//! and reads the modes that were published with it to encode input.
//!
//! The only thing shared is that published screen, and it is locked for as
//! long as it takes to copy the lines that changed. Parsing, which can take a
//! while, always happens outside of any lock.
//!
//! Going the other way, the ui doesn't touch the emulator either. It leaves
//! what it wants (a new size, a scroll position) in `Shared`, and the `Core`
//! applies it before it next publishes.

use std::{
  sync::{
    Arc, Mutex, MutexGuard, PoisonError,
    atomic::{AtomicUsize, Ordering},
  },
  time::Instant,
};

use crate::{
  Emulator, Size,
  screen::{Lines, Modes, Screen},
};

/// What the ui has asked the reader to change.
#[derive(Default)]
struct Control {
  resize: Option<Size>,
  colors: Option<([u8; 3], [u8; 3])>,
}

pub(crate) struct Shared {
  // The most recent screen. Both sides lock this only to copy.
  published: Mutex<Screen>,
  control:   Mutex<Control>,

  // How far back the ui wants to be scrolled. The reader applies this when it
  // publishes.
  viewport: AtomicUsize,
}

impl Shared {
  pub fn new() -> Arc<Shared> {
    Arc::new(Shared {
      published: Mutex::new(Screen::new()),
      control:   Mutex::new(Control::default()),
      viewport:  AtomicUsize::new(0),
    })
  }

  fn published(&self) -> MutexGuard<'_, Screen> {
    self.published.lock().unwrap_or_else(PoisonError::into_inner)
  }

  fn control(&self) -> MutexGuard<'_, Control> {
    self.control.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// Copies the published screen into `screen`.
  pub fn snapshot(&self, screen: &mut Screen) { screen.capture_from(&self.published()); }

  pub fn modes(&self) -> Modes { self.published().modes }
  pub fn scrollback_len(&self) -> usize { self.published().scrollback_len() }

  /// How far back the ui is scrolled, or is about to be.
  pub fn viewport_offset(&self) -> usize {
    self.viewport.load(Ordering::Relaxed).min(self.scrollback_len())
  }

  /// Scrolls back by `dy` lines (forward if negative), within scrollback.
  pub fn scroll(&self, dy: i32) {
    let max = self.scrollback_len();
    let offset = self.viewport.load(Ordering::Relaxed).min(max);
    self.viewport.store(offset.saturating_add_signed(dy as isize).min(max), Ordering::Relaxed);
  }

  pub fn request_resize(&self, size: Size) { self.control().resize = Some(size); }

  pub fn request_colors(&self, foreground: [u8; 3], background: [u8; 3]) {
    self.control().colors = Some((foreground, background));
  }
}

/// The emulator, and the way it gets onto the screen. Owned by whoever reads
/// the pty.
pub(crate) struct Core {
  pub emu: Emulator,
  shared:  Arc<Shared>,

  // Whether the emulator has changed since it was last published.
  changed: bool,
  // How far back the last published screen was scrolled.
  offset:  usize,
}

impl Core {
  /// Makes an emulator, and publishes its blank screen.
  pub fn new(size: Size, shared: Arc<Shared>) -> Core {
    let mut core = Core { emu: Emulator::new(size), shared, changed: true, offset: 0 };
    core.publish();
    core
  }

  /// Parses output, and returns the bytes the program is waiting for, which
  /// the caller should send to the pty. Call `publish` afterwards.
  pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
    self.emu.consume(bytes);
    self.emu.release_held();
    self.changed = true;
    self.replies()
  }

  /// Releases held output (see `Emulator::hold_deadline`) if it is due.
  pub fn tick(&mut self) -> Vec<u8> {
    self.emu.release_held();
    self.changed = true;
    self.replies()
  }

  /// When held output must be released, even if the program says nothing more.
  pub fn deadline(&self) -> Option<Instant> { self.emu.hold_deadline }

  fn replies(&mut self) -> Vec<u8> {
    if self.emu.state.has_replies() { self.emu.take_replies() } else { Vec::new() }
  }

  /// Applies what the ui has asked for. Returns the new size, if the ui asked
  /// for one, so that the caller can tell the pty.
  pub fn apply_control(&mut self) -> Option<Size> {
    let control = std::mem::take(&mut *self.shared.control());

    if let Some((foreground, background)) = control.colors {
      self.emu.state.default_foreground = foreground;
      self.emu.state.default_background = background;
    }

    let resized = control.resize.filter(|&size| size != self.emu.size());
    if let Some(size) = resized {
      self.emu.resize(size);
      self.changed = true;
    }

    if self.shared.viewport.load(Ordering::Relaxed).min(self.emu.state.scrollback.len())
      != self.offset
    {
      self.changed = true;
    }

    resized
  }

  /// Copies what has changed onto the published screen, and wakes the ui. Does
  /// nothing if nothing has changed.
  pub fn publish(&mut self) {
    if !self.changed {
      return;
    }
    self.changed = false;

    self.offset = self.shared.viewport.load(Ordering::Relaxed).min(self.emu.state.scrollback.len());
    self.emu.state.viewport_offset = self.offset;

    {
      let mut screen = self.shared.published();
      screen.capture(&self.emu.state);
      screen.modes = Modes::of(&self.emu);
    }

    // Lines written from now on get a new stamp, so that the copies can tell
    // they changed.
    self.emu.state.begin_frame();
  }

  /// Parses everything that is held back, and publishes it. For when the pty
  /// has closed.
  pub fn close(&mut self) {
    self.emu.finish_hold();
    self.changed = true;
    self.publish();
  }
}
