//! Substring search. Both directions walk the chunks, searching each one as a
//! plain `&str`, so nothing ever indexes the tree by byte. A match that
//! straddles chunks is found by keeping up to `pattern.len() - 1` bytes of the
//! neighboring text in a small buffer.

use std::collections::VecDeque;

use crate::{RopeSlice, iter::Chunks};

fn floor_boundary(s: &str, mut at: usize) -> usize {
  while !s.is_char_boundary(at) {
    at -= 1;
  }
  at
}

fn ceil_boundary(s: &str, mut at: usize) -> usize {
  while !s.is_char_boundary(at) {
    at += 1;
  }
  at
}

/// The non-overlapping matches of a pattern, front to back, as byte offsets
/// into the slice. Made by [`RopeSlice::find`].
///
/// Matches are found greedily, so the same ones as [`str::match_indices`].
pub struct Find<'a, 'p> {
  chunks:    Chunks<'a>,
  pattern:   &'p str,
  /// The offset of the end of the chunks read so far.
  pos:       usize,
  /// The end of the text read so far, at most `pattern.len() - 1` bytes of it:
  /// the part a match could still start in.
  carry:     String,
  /// Matches can't start before this, or they would overlap the last one.
  min_start: usize,
  queue:     VecDeque<usize>,
}

impl<'a, 'p> Find<'a, 'p> {
  pub(crate) fn new(slice: RopeSlice<'a>, pattern: &'p str) -> Find<'a, 'p> {
    Find {
      chunks: slice.chunks(),
      pattern,
      pos: 0,
      carry: String::new(),
      min_start: 0,
      queue: VecDeque::new(),
    }
  }

  fn scan(&mut self, chunk: &str) {
    let plen = self.pattern.len();
    let start = self.pos;
    self.pos += chunk.len();

    // Matches that start in the carried text and end in this chunk.
    if !self.carry.is_empty() {
      let take = floor_boundary(chunk, (plen - 1).min(chunk.len()));
      let mut buf = self.carry.clone();
      buf.push_str(&chunk[..take]);

      let mut from = 0;
      while let Some(i) = buf[from..].find(self.pattern) {
        let at = from + i;
        if at >= self.carry.len() {
          // Wholly inside the chunk, the next step finds it.
          break;
        }
        self.queue.push_back(start - self.carry.len() + at);
        from = at + plen;
        self.min_start = start - self.carry.len() + from;
      }
    }

    // Matches inside the chunk.
    let mut from = self.min_start.saturating_sub(start).min(chunk.len());
    while let Some(i) = chunk[from..].find(self.pattern) {
      self.queue.push_back(start + from + i);
      from += i + plen;
      self.min_start = start + from;
    }

    // Keep the end of the text for the next chunk. A chunk can be shorter than
    // that, in which case the carry runs on into it.
    if plen == 1 {
      return;
    }
    let mut full = if chunk.len() >= plen - 1 { String::new() } else { self.carry.clone() };
    full.push_str(chunk);
    let keep = ceil_boundary(&full, full.len() - (plen - 1).min(full.len()));
    self.carry = full[keep..].to_string();

    let carry_start = self.pos - self.carry.len();
    if carry_start < self.min_start {
      self.carry.drain(..self.min_start - carry_start);
    }
  }
}

impl Iterator for Find<'_, '_> {
  type Item = usize;

  fn next(&mut self) -> Option<usize> {
    if self.pattern.is_empty() {
      return None;
    }
    loop {
      if let Some(at) = self.queue.pop_front() {
        return Some(at);
      }
      let chunk = self.chunks.next()?;
      self.scan(chunk);
    }
  }
}

/// The non-overlapping matches of a pattern, back to front, as byte offsets
/// into the slice. Made by [`RopeSlice::rfind`].
///
/// Matches are found greedily from the end, so the same ones as
/// [`str::rmatch_indices`].
pub struct RFind<'a, 'p> {
  chunks:  Chunks<'a>,
  pattern: &'p str,
  /// The offset of the start of the chunks read so far.
  pos:     usize,
  /// The start of the text read so far, at most `pattern.len() - 1` bytes of
  /// it: the part a match could still end in.
  carry:   String,
  /// Matches can't end after this, or they would overlap the last one.
  max_end: usize,
  queue:   VecDeque<usize>,
}

impl<'a, 'p> RFind<'a, 'p> {
  pub(crate) fn new(slice: RopeSlice<'a>, pattern: &'p str) -> RFind<'a, 'p> {
    RFind {
      chunks: slice.chunks(),
      pattern,
      pos: slice.byte_len(),
      carry: String::new(),
      max_end: slice.byte_len(),
      queue: VecDeque::new(),
    }
  }

  fn scan(&mut self, chunk: &str) {
    let plen = self.pattern.len();
    let start = self.pos - chunk.len();
    self.pos = start;

    // Matches that start in this chunk and end in the carried text.
    if !self.carry.is_empty() {
      let take = ceil_boundary(chunk, chunk.len() - (plen - 1).min(chunk.len()));
      let tail = &chunk[take..];
      let mut buf = tail.to_string();
      buf.push_str(&self.carry);
      let buf_start = start + take;

      let mut limit = (self.max_end - buf_start).min(buf.len());
      while let Some(at) = buf[..limit].rfind(self.pattern) {
        if at + plen <= tail.len() {
          // Wholly inside the chunk, the next step finds it.
          break;
        }
        self.queue.push_back(buf_start + at);
        limit = at;
        self.max_end = buf_start + at;
      }
    }

    // Matches inside the chunk.
    let mut limit = self.max_end.saturating_sub(start).min(chunk.len());
    while let Some(at) = chunk[..limit].rfind(self.pattern) {
      self.queue.push_back(start + at);
      limit = at;
      self.max_end = start + at;
    }

    // Keep the start of the text for the next chunk.
    if plen == 1 {
      return;
    }
    let mut full = chunk.to_string();
    if chunk.len() < plen - 1 {
      full.push_str(&self.carry);
    }
    let keep = floor_boundary(&full, (plen - 1).min(full.len()));
    full.truncate(keep);
    self.carry = full;

    if start + self.carry.len() > self.max_end {
      self.carry.truncate(self.max_end - start);
    }
  }
}

impl Iterator for RFind<'_, '_> {
  type Item = usize;

  fn next(&mut self) -> Option<usize> {
    if self.pattern.is_empty() {
      return None;
    }
    loop {
      if let Some(at) = self.queue.pop_front() {
        return Some(at);
      }
      let chunk = self.chunks.next_back()?;
      self.scan(chunk);
    }
  }
}
