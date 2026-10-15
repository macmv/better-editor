//! A rope: a persistent text buffer built on [`be_sumtree`].
//!
//! Text is stored in chunks, and every tree node caches a summary of its
//! chunks. The summary counts bytes, characters, UTF-16 code units, lines, and
//! grapheme clusters, so converting between any two of them, and finding the
//! length of any line, costs O(log n) no matter how long the lines are.

pub mod grapheme;
pub mod iter;
mod rope;
mod search;
mod slice;
mod summary;

pub use rope::{Rope, RopeBuilder};
pub use search::{Find, RFind};
pub use slice::RopeSlice;

#[cfg(test)]
mod tests;
