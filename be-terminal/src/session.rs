//! How a terminal's output gets from the pty to the ui.
//!
//! Whoever reads the pty (a thread, or the ui for hosts that can't leave its
//! thread) owns a `Core`: the emulator, and nothing else touches it. After it
//! parses output, it publishes what changed by copying it into a `Screen` in
//! `Shared`, and wakes the ui. The ui copies that into its own `Screen` when it
//! is about to draw.
//!
//! The only lock the two sides share is around that published screen, and it
//! is held for as long as it takes to copy the lines that changed. That is
//! what keeps a keystroke fast: the reader parses an echoed byte, copies one
//! line, and wakes the ui, and neither side ever waits for the other to parse.
//! Parsing, which can take a while, always happens outside of any lock.
//!
//! Going the other way, the ui doesn't touch the emulator either. It leaves
//! what it wants (a new size, a scroll position) in `Shared`, and pokes the
//! reader, which applies it before it next publishes.

use std::{
  fs::File,
  io::{self, Read, Write},
  sync::{
    Arc, Mutex, MutexGuard, PoisonError,
    atomic::{AtomicBool, AtomicUsize, Ordering},
  },
  thread::JoinHandle,
  time::Instant,
};

use crate::{
  Emulator, READ_SIZE, Size,
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

  // Set when something has been published since the ui last took a snapshot,
  // and the ui was told. Wakes are only sent when this goes from false to true,
  // so there is at most one per frame.
  dirty:  AtomicBool,
  closed: AtomicBool,
  wake:   Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Shared {
  pub fn new() -> Arc<Shared> {
    Arc::new(Shared {
      published: Mutex::new(Screen::new()),
      control:   Mutex::new(Control::default()),
      viewport:  AtomicUsize::new(0),
      dirty:     AtomicBool::new(false),
      closed:    AtomicBool::new(false),
      wake:      Mutex::new(None),
    })
  }

  fn published(&self) -> MutexGuard<'_, Screen> {
    self.published.lock().unwrap_or_else(PoisonError::into_inner)
  }

  fn control(&self) -> MutexGuard<'_, Control> {
    self.control.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// Copies the published screen into `screen`, and tells the reader that the
  /// next change should wake the ui again.
  pub fn snapshot(&self, screen: &mut Screen) {
    self.dirty.store(false, Ordering::Release);
    screen.capture_from(&self.published());
  }

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

  pub fn is_closed(&self) -> bool { self.closed.load(Ordering::Acquire) }

  /// Tells the ui there is something new to draw, if it doesn't already know.
  fn notify(&self) {
    if self.dirty.swap(true, Ordering::AcqRel) {
      return;
    }

    match &*self.wake.lock().unwrap_or_else(PoisonError::into_inner) {
      Some(wake) => wake(),
      // Nobody to tell yet. `set_wake` will tell them.
      None => self.dirty.store(false, Ordering::Release),
    }
  }

  pub fn set_wake(&self, wake: Box<dyn Fn() + Send>) {
    let mut slot = self.wake.lock().unwrap_or_else(PoisonError::into_inner);
    // Output may have come in before there was anyone to wake.
    wake();
    *slot = Some(wake);
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
    self.shared.notify();
  }

  /// Parses everything that is held back, publishes it, and marks the terminal
  /// closed. For when the pty has closed.
  pub fn close(&mut self) {
    self.emu.finish_hold();
    self.changed = true;
    self.publish();

    self.shared.closed.store(true, Ordering::Release);
    self.shared.notify();
  }
}

/// Input to the pty that couldn't be sent right away.
///
/// Both the ui (keys) and the reader (replies) send through this, so the bytes
/// go out in the order they were written, and neither ever blocks: what the
/// pty won't take yet waits here until the reader sees that it is writable.
pub(crate) struct Writer {
  file:   File,
  queue:  Mutex<Vec<u8>>,
  poller: Arc<polling::Poller>,
}

impl Writer {
  pub fn write(&self, bytes: &[u8]) {
    if bytes.is_empty() {
      return;
    }

    let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
    queue.extend_from_slice(bytes);
    self.flush_locked(&mut queue);

    if !queue.is_empty() {
      // Have the reader wait for the pty to be writable.
      let _ = self.poller.notify();
    }
  }

  pub fn flush(&self) {
    let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
    self.flush_locked(&mut queue);
  }

  pub fn pending(&self) -> bool {
    !self.queue.lock().unwrap_or_else(PoisonError::into_inner).is_empty()
  }

  fn flush_locked(&self, queue: &mut Vec<u8>) {
    while !queue.is_empty() {
      match (&self.file).write(queue) {
        Ok(n) => {
          queue.drain(..n);
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
        Err(e) => {
          // This happens when the program has exited, which isn't a bug.
          warn!("failed to write to pty: {e}");
          queue.clear();
          break;
        }
      }
    }
  }
}

/// A thread that reads a pty into a `Core`.
pub(crate) struct ReaderThread {
  poller: Arc<polling::Poller>,
  stop:   Arc<AtomicBool>,
  thread: Option<JoinHandle<()>>,
}

impl ReaderThread {
  /// Starts reading `file`, which must be nonblocking. Returns the thread and
  /// the way to write to the same pty.
  pub fn spawn(file: File, core: Core) -> io::Result<(ReaderThread, Arc<Writer>)> {
    let poller = Arc::new(polling::Poller::new()?);
    let stop = Arc::new(AtomicBool::new(false));
    let writer = Arc::new(Writer {
      file:   file.try_clone()?,
      queue:  Mutex::new(Vec::new()),
      poller: poller.clone(),
    });

    // SAFETY: `file` is deleted from the poller before the thread exits.
    unsafe { poller.add(&file, polling::Event::readable(0))? };

    let thread = {
      let poller = poller.clone();
      let stop = stop.clone();
      let writer = writer.clone();
      std::thread::Builder::new()
        .name("pty reader".into())
        .spawn(move || run(file, core, writer, poller, stop))?
    };

    Ok((ReaderThread { poller, stop, thread: Some(thread) }, writer))
  }

  /// Makes the reader look at what the ui has asked for (see `Shared`).
  pub fn poke(&self) { let _ = self.poller.notify(); }
}

impl Drop for ReaderThread {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Release);
    let _ = self.poller.notify();
    if let Some(thread) = self.thread.take() {
      let _ = thread.join();
    }
  }
}

fn run(
  file: File,
  mut core: Core,
  writer: Arc<Writer>,
  poller: Arc<polling::Poller>,
  stop: Arc<AtomicBool>,
) {
  let mut buf = vec![0; READ_SIZE];
  let mut events = polling::Events::new();

  'outer: while !stop.load(Ordering::Acquire) {
    // The poller is oneshot, so interest is set again every time around.
    if poller.modify(&file, polling::Event::new(0, true, writer.pending())).is_err() {
      break;
    }

    // Held output has to be released on time, even if the program says nothing
    // more.
    let timeout = core.deadline().map(|d| d.saturating_duration_since(Instant::now()));
    events.clear();
    match poller.wait(&mut events, timeout) {
      Ok(_) => {}
      Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
      Err(e) => {
        error!("failed to poll pty: {e}");
        break;
      }
    }

    if let Some(size) = core.apply_control() {
      crate::pty::set_winsize(&file, size);
    }
    writer.flush();

    loop {
      if stop.load(Ordering::Acquire) {
        break 'outer;
      }

      match (&file).read(&mut buf) {
        // Ok(0) -> pty closed on macos
        Ok(0) => {
          core.close();
          break 'outer;
        }
        // EIO error -> pty closed on linux
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) => {
          core.close();
          break 'outer;
        }
        Ok(n) => {
          writer.write(&core.feed(&buf[..n]));
          // After every read, so that a program that never stops printing still
          // shows up.
          core.publish();
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
        Err(e) => {
          error!("failed to read from pty: {e}");
          core.close();
          break 'outer;
        }
      }
    }

    if core.deadline().is_some_and(|d| Instant::now() >= d) {
      writer.write(&core.tick());
    }
    // Anything the ui asked for, and held output that was just released.
    core.publish();
  }

  let _ = poller.delete(&file);
}
