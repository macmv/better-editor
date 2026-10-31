//! The pure parts of the shell view: which blocks exist and whether they are
//! collapsed, and where they sit in the scrolled list. Nothing here touches
//! rendering.

use be_shell::JobId;
use be_terminal::Emulator;
use kurbo::{Point, Size};

use super::terminal_paint::grid_size;

/// The key of the live prompt at the bottom of the list, which isn't a block.
pub const PROMPT_KEY: u64 = u64::MAX;

pub enum BlockKind {
  Job(JobId),
  /// The output of the shell's startup (what the rc files printed), which has
  /// no prompt and is never collapsed.
  Startup(JobId),
  /// A message from the view itself, such as "jobs: not supported yet".
  Notice(Vec<String>),
}

pub struct Block {
  /// Never reused, and not changed by collapsing or clearing.
  pub key:       u64,
  pub kind:      BlockKind,
  pub collapsed: bool,
}

/// The blocks above the prompt, oldest first, and which one has keyboard focus.
#[derive(Default)]
pub struct Blocks {
  list:     Vec<Block>,
  next_key: u64,
  focus:    Option<u64>,
}

impl Blocks {
  pub fn list(&self) -> &[Block] { &self.list }
  pub fn focus(&self) -> Option<u64> { self.focus }

  pub fn push(&mut self, kind: BlockKind) -> u64 {
    let key = self.next_key;
    self.next_key += 1;
    self.list.push(Block { key, kind, collapsed: false });
    key
  }

  /// Hides every block.
  pub fn clear(&mut self) {
    self.list.clear();
    self.focus = None;
  }

  pub fn get(&self, key: u64) -> Option<&Block> { self.list.iter().find(|b| b.key == key) }

  pub fn job_block(&self, job: JobId) -> Option<&Block> {
    self.list.iter().find(|b| matches!(b.kind, BlockKind::Job(j) if j == job))
  }

  pub fn set_collapsed(&mut self, key: u64, collapsed: bool) {
    if let Some(b) = self.list.iter_mut().find(|b| b.key == key)
      && matches!(b.kind, BlockKind::Job(_))
    {
      b.collapsed = collapsed;
    }
  }

  pub fn toggle(&mut self, key: u64) {
    if let Some(b) = self.get(key) {
      self.set_collapsed(key, !b.collapsed);
    }
  }

  pub fn set_all_collapsed(&mut self, collapsed: bool) {
    for b in &mut self.list {
      if matches!(b.kind, BlockKind::Job(_)) {
        b.collapsed = collapsed;
      }
    }
  }

  pub fn set_focus(&mut self, key: Option<u64>) {
    self.focus = key.filter(|k| self.get(*k).is_some());
  }

  /// Focuses the newest block if nothing is focused.
  pub fn ensure_focus(&mut self) {
    if self.focus.and_then(|k| self.get(k)).is_none() {
      self.focus = self.list.last().map(|b| b.key);
    }
  }

  /// Moves focus `delta` blocks down (positive) or up, stopping at the ends.
  pub fn move_focus(&mut self, delta: isize) {
    self.ensure_focus();
    let Some(key) = self.focus else { return };
    let i = self.list.iter().position(|b| b.key == key).unwrap_or(0) as isize;
    let i = (i + delta).clamp(0, self.list.len() as isize - 1);
    self.focus = Some(self.list[i as usize].key);
  }
}

/// Whether a job takes over the whole view: it is running and has switched to
/// the alternate screen. A finished job never does, even if it died without
/// leaving the alternate screen.
pub fn is_fullscreen(running: bool, emulator: &Emulator) -> bool {
  running && emulator.alt_screen()
}

/// The size of the pty for a view of `size` with cells of `char_size`. In a
/// fullscreen takeover that is the whole view; otherwise it is the text area
/// minus `pad_right`, and big enough to be usable.
pub fn pty_grid(
  size: Size,
  char_size: Size,
  pad_right: f64,
  fullscreen: bool,
) -> be_terminal::Size {
  if fullscreen {
    let grid = grid_size(size, char_size);
    be_terminal::Size { rows: grid.rows.max(1), cols: grid.cols.max(1) }
  } else {
    let grid = grid_size(Size::new((size.width - pad_right).max(0.0), size.height), char_size);
    be_terminal::Size { rows: grid.rows.max(5), cols: grid.cols.max(10) }
  }
}

/// The `(col, row)` of the cell under `pos` in a fullscreen view.
pub fn mouse_cell(pos: Point, char_size: Size) -> (usize, usize) {
  let cell = |v: f64, size: f64| (v / size).floor().max(0.0) as usize;
  (cell(pos.x, char_size.width), cell(pos.y, char_size.height))
}

/// Where a block or the prompt sits in the scrolled content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
  pub key:    u64,
  pub top:    f64,
  pub height: f64,
}

/// Stacks items of the given heights from the top. Returns the items and
/// their total height.
pub fn stack(heights: impl IntoIterator<Item = (u64, f64)>) -> (Vec<Item>, f64) {
  let mut top = 0.0;
  let items = heights
    .into_iter()
    .map(|(key, height)| {
      let item = Item { key, top, height };
      top += height;
      item
    })
    .collect();
  (items, top)
}

#[derive(Clone, Copy, Debug)]
struct Anchor {
  key:   u64,
  /// How far into the item the top of the viewport is.
  delta: f64,
}

/// The scroll position of the list.
///
/// The view follows the bottom until the user scrolls up. After that, the top
/// of the viewport is pinned to a point inside the item it was over, so that
/// items above it growing or shrinking (collapsing, output arriving) don't
/// shift what is on screen.
#[derive(Debug)]
pub struct ScrollState {
  follow: bool,
  anchor: Option<Anchor>,
  offset: f64,
}

impl Default for ScrollState {
  fn default() -> Self { ScrollState { follow: true, anchor: None, offset: 0.0 } }
}

fn anchor_at(offset: f64, items: &[Item]) -> Option<Anchor> {
  let item = items.iter().rev().find(|i| i.top <= offset).or(items.first())?;
  Some(Anchor { key: item.key, delta: offset - item.top })
}

impl ScrollState {
  #[cfg(test)]
  pub fn following(&self) -> bool { self.follow }

  /// The distance from the top of the content to the top of the viewport.
  pub fn offset(&self) -> f64 { self.offset }

  /// Recomputes the offset after the items changed.
  pub fn update(&mut self, items: &[Item], total: f64, viewport: f64) -> f64 {
    let max = (total - viewport).max(0.0);
    if self.follow {
      self.offset = max;
    } else {
      let pinned = self
        .anchor
        .and_then(|a| items.iter().find(|i| i.key == a.key).map(|i| i.top + a.delta.min(i.height)));
      self.offset = pinned.unwrap_or(self.offset).clamp(0.0, max);
      if pinned.is_none() {
        self.anchor = anchor_at(self.offset, items);
      }
    }
    self.offset
  }

  /// Scrolls by `dy` pixels (positive is down). Reaching the bottom resumes
  /// following.
  pub fn scroll_by(&mut self, dy: f64, items: &[Item], total: f64, viewport: f64) {
    let max = (total - viewport).max(0.0);
    self.set_offset((self.offset + dy).clamp(0.0, max), items, max);
  }

  /// Scrolls the minimum distance to show all of `key` (or its top, if it is
  /// taller than the viewport).
  pub fn reveal(&mut self, key: u64, items: &[Item], total: f64, viewport: f64) {
    let Some(item) = items.iter().find(|i| i.key == key) else { return };
    let max = (total - viewport).max(0.0);
    let bottom = item.top + item.height;

    let offset = if item.top < self.offset || item.height > viewport {
      item.top
    } else if bottom > self.offset + viewport {
      bottom - viewport
    } else {
      self.offset
    };
    self.set_offset(offset.clamp(0.0, max), items, max);
  }

  pub fn follow_bottom(&mut self) { self.follow = true; }

  fn set_offset(&mut self, offset: f64, items: &[Item], max: f64) {
    self.offset = offset;
    self.follow = offset >= max - 0.5;
    self.anchor = anchor_at(offset, items);
  }
}

/// A command for display in a one-line header.
pub fn header_command(line: &str) -> String {
  line.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn items() -> (Vec<Item>, f64) { stack([(0, 100.0), (1, 50.0), (2, 200.0), (PROMPT_KEY, 40.0)]) }

  #[test]
  fn stacking() {
    let (items, total) = items();
    assert_eq!(total, 390.0);
    assert_eq!(items[2], Item { key: 2, top: 150.0, height: 200.0 });
  }

  #[test]
  fn follows_bottom_until_scrolled() {
    let (items, total) = items();
    let mut s = ScrollState::default();
    assert_eq!(s.update(&items, total, 100.0), 290.0);
    assert!(s.following());

    s.scroll_by(-60.0, &items, total, 100.0);
    assert!(!s.following());
    assert_eq!(s.offset(), 230.0);

    // More output at the bottom does not move the view.
    let (grown, grown_total) = stack([(0, 100.0), (1, 50.0), (2, 300.0), (PROMPT_KEY, 40.0)]);
    assert_eq!(s.update(&grown, grown_total, 100.0), 230.0);

    // Scrolling back to the bottom resumes following.
    s.scroll_by(10_000.0, &grown, grown_total, 100.0);
    assert!(s.following());
    assert_eq!(s.offset(), 390.0);
  }

  #[test]
  fn collapsing_above_the_viewport_does_not_jump() {
    let (items, total) = items();
    let mut s = ScrollState::default();
    s.update(&items, total, 100.0);
    s.scroll_by(-100.0, &items, total, 100.0);
    // The viewport top is 190, 40px into item 2.
    assert_eq!(s.offset(), 190.0);

    // Item 0 collapses from 100 to 20: item 2 moves up by 80.
    let (collapsed, collapsed_total) =
      stack([(0, 20.0), (1, 50.0), (2, 200.0), (PROMPT_KEY, 40.0)]);
    assert_eq!(s.update(&collapsed, collapsed_total, 100.0), 110.0);
  }

  #[test]
  fn anchor_clamps_when_the_item_shrinks() {
    let (items, total) = items();
    let mut s = ScrollState::default();
    s.update(&items, total, 100.0);
    s.scroll_by(-100.0, &items, total, 100.0);

    let (small, small_total) = stack([(0, 100.0), (1, 50.0), (2, 10.0), (PROMPT_KEY, 40.0)]);
    // Pinned 40px into a 10px item: stays at the end of it, then clamps.
    assert_eq!(s.update(&small, small_total, 100.0), 100.0);
  }

  #[test]
  fn clearing_keeps_the_offset_valid() {
    let (items, total) = items();
    let mut s = ScrollState::default();
    s.update(&items, total, 100.0);
    s.scroll_by(-50.0, &items, total, 100.0);

    let (only_prompt, total) = stack([(PROMPT_KEY, 40.0)]);
    assert_eq!(s.update(&only_prompt, total, 100.0), 0.0);
  }

  #[test]
  fn reveal_scrolls_minimally() {
    let (items, total) = items();
    let mut s = ScrollState::default();
    s.update(&items, total, 100.0);
    s.reveal(1, &items, total, 100.0);
    assert_eq!(s.offset(), 100.0);
    assert!(!s.following());

    // Already fully visible: no change.
    s.reveal(1, &items, total, 100.0);
    assert_eq!(s.offset(), 100.0);

    // Below the viewport: bottom aligns.
    s.reveal(2, &items, total, 100.0);
    assert_eq!(s.offset(), 150.0);
    s.scroll_by(-150.0, &items, total, 100.0);
    s.reveal(0, &items, total, 300.0);
    assert_eq!(s.offset(), 0.0);
  }

  #[test]
  fn collapse_state() {
    let mut blocks = Blocks::default();
    let a = blocks.push(BlockKind::Job(JobId(1)));
    let n = blocks.push(BlockKind::Notice(vec!["hi".into()]));
    let b = blocks.push(BlockKind::Job(JobId(2)));

    blocks.toggle(a);
    assert!(blocks.get(a).unwrap().collapsed);
    blocks.toggle(n);
    assert!(!blocks.get(n).unwrap().collapsed);

    blocks.set_all_collapsed(true);
    assert!(blocks.get(b).unwrap().collapsed);
    assert!(!blocks.get(n).unwrap().collapsed);
    blocks.set_all_collapsed(false);
    assert!(!blocks.get(a).unwrap().collapsed);
    assert_eq!(blocks.job_block(JobId(2)).unwrap().key, b);
  }

  #[test]
  fn focus_moves_and_stops_at_ends() {
    let mut blocks = Blocks::default();
    blocks.move_focus(1);
    assert_eq!(blocks.focus(), None);

    let a = blocks.push(BlockKind::Job(JobId(1)));
    let b = blocks.push(BlockKind::Job(JobId(2)));
    blocks.ensure_focus();
    assert_eq!(blocks.focus(), Some(b));
    blocks.move_focus(1);
    assert_eq!(blocks.focus(), Some(b));
    blocks.move_focus(-1);
    assert_eq!(blocks.focus(), Some(a));
    blocks.move_focus(-1);
    assert_eq!(blocks.focus(), Some(a));

    blocks.clear();
    assert_eq!(blocks.focus(), None);
  }

  #[test]
  fn alt_screen_round_trip() {
    let mut emulator = Emulator::new(be_terminal::Size { rows: 5, cols: 20 });
    emulator.feed(b"normal output\r\n");
    assert!(!is_fullscreen(true, &emulator));

    emulator.feed(b"\x1b[?1049h\x1b[Halt");
    assert!(is_fullscreen(true, &emulator));
    // A job that is done never takes over the view.
    assert!(!is_fullscreen(false, &emulator));

    emulator.feed(b"\x1b[?1049l");
    assert!(!is_fullscreen(true, &emulator));
    // The normal screen is as it was.
    assert_eq!(emulator.text(), "normal output");
  }

  #[test]
  fn grid_sizes() {
    let cell = Size::new(10.0, 20.0);
    let view = Size::new(805.0, 410.0);
    assert_eq!(pty_grid(view, cell, 8.0, false), be_terminal::Size { rows: 20, cols: 79 });
    assert_eq!(pty_grid(view, cell, 8.0, true), be_terminal::Size { rows: 20, cols: 80 });
    // Tiny views stay usable for a shell but not clamped when fullscreen.
    assert_eq!(
      pty_grid(Size::new(30.0, 30.0), cell, 8.0, false),
      be_terminal::Size { rows: 5, cols: 10 }
    );
    assert_eq!(
      pty_grid(Size::new(30.0, 30.0), cell, 8.0, true),
      be_terminal::Size { rows: 1, cols: 3 }
    );
  }

  #[test]
  fn mouse_cells() {
    let cell = Size::new(10.0, 20.0);
    assert_eq!(mouse_cell(Point::new(0.0, 0.0), cell), (0, 0));
    assert_eq!(mouse_cell(Point::new(19.9, 39.9), cell), (1, 1));
    assert_eq!(mouse_cell(Point::new(-5.0, 41.0), cell), (0, 2));
  }

  #[test]
  fn header_command_is_one_line() {
    assert_eq!(header_command("echo a\n  echo b\n"), "echo a echo b");
  }
}
