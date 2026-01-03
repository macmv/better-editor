#![allow(dead_code)]

use be_doc::Document;
use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

struct ColorLinePrinter<'a>(&'a imara_diff::Interner<&'a str>);
use std::{fmt, hash::Hash, ops::Range};

struct CharTokens<'a>(&'a str);

impl<'a> TokenSource for CharTokens<'a> {
  type Token = char;
  type Tokenizer = std::str::Chars<'a>;

  fn tokenize(&self) -> Self::Tokenizer { self.0.chars() }

  fn estimate_tokens(&self) -> u32 { self.0.len() as u32 }
}

pub struct LineDiff {
  diff: Diff,
}

pub fn line_diff<'a>(before: &Document, after: &Document) -> LineDiff {
  let input = InternedInput::new(DocLines(before), DocLines(after));
  let mut diff = Diff::compute(Algorithm::Histogram, &input);
  diff.postprocess_no_heuristic(&input);

  LineDiff { diff }
}

impl LineDiff {
  pub fn changes(&self) -> impl Iterator<Item = Range<usize>> {
    self.diff.hunks().map(|hunk| hunk.after.start as usize..hunk.after.end as usize)
  }
}

struct DocLines<'a>(&'a Document);
#[derive(PartialEq, Eq)]
struct RopeSliceHash<'a>(be_doc::crop::RopeSlice<'a>);

impl Hash for RopeSliceHash<'_> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    // Like `state.write_str`.
    for chunk in self.0.chunks() {
      state.write(chunk.as_bytes());
    }
    state.write_u8(0xff);
  }
}

impl<'a> TokenSource for DocLines<'a> {
  type Token = RopeSliceHash<'a>;
  type Tokenizer = std::iter::Map<
    be_doc::crop::iter::Lines<'a>,
    fn(be_doc::crop::RopeSlice<'a>) -> RopeSliceHash<'a>,
  >;

  fn tokenize(&self) -> Self::Tokenizer { self.0.rope.lines().map(|l| RopeSliceHash(l)) }

  fn estimate_tokens(&self) -> u32 {
    // Like imara_diff::ByteLines, but we don't actually read anything.
    100
  }
}

impl imara_diff::UnifiedDiffPrinter for ColorLinePrinter<'_> {
  fn display_header(
    &self,
    mut f: impl fmt::Write,
    start_before: u32,
    start_after: u32,
    len_before: u32,
    len_after: u32,
  ) -> fmt::Result {
    writeln!(f, "@@ -{},{} +{},{} @@", start_before + 1, len_before, start_after + 1, len_after)
  }

  fn display_context_token(&self, mut f: impl fmt::Write, token: imara_diff::Token) -> fmt::Result {
    write!(f, " {}", &self.0[token])?;
    if !&self.0[token].ends_with('\n') {
      writeln!(f)?;
    }
    Ok(())
  }

  fn display_hunk(
    &self,
    mut f: impl fmt::Write,
    before: &[imara_diff::Token],
    after: &[imara_diff::Token],
  ) -> fmt::Result {
    if before.len() == 1 && after.len() == 1 {
      let before = self.0[before[0]];
      let after = self.0[after[0]];

      let input = InternedInput::new(CharTokens(before), CharTokens(after));
      let mut diff = Diff::compute(Algorithm::Histogram, &input);
      diff.postprocess_no_heuristic(&input);

      let mut prev = 0;
      write!(f, "\x1b[31m-")?;
      for hunk in diff.hunks() {
        if hunk.before.start as usize > prev {
          for &c in &input.before[prev..hunk.before.start as usize] {
            write!(f, "{}", input.interner[c])?;
          }
        }

        write!(f, "\x1b[48;2;64;0;0m")?;
        for &c in &input.before[hunk.before.start as usize..hunk.before.end as usize] {
          write!(f, "{}", input.interner[c])?;
        }
        write!(f, "\x1b[49m")?;
        prev = hunk.before.end as usize;
      }
      if prev < after.len() {
        for &c in &input.before[prev as usize..] {
          write!(f, "{}", input.interner[c])?;
        }
      }

      let mut prev = 0;
      write!(f, "\x1b[32m+")?;
      for hunk in diff.hunks() {
        if hunk.after.start as usize > prev {
          for &c in &input.after[prev..hunk.after.start as usize] {
            write!(f, "{}", input.interner[c])?;
          }
        }

        write!(f, "\x1b[48;2;0;64;0m")?;
        for &c in &input.after[hunk.after.start as usize..hunk.after.end as usize] {
          write!(f, "{}", input.interner[c])?;
        }
        write!(f, "\x1b[49m")?;
        prev = hunk.after.end as usize;
      }
      if prev < after.len() {
        for &c in &input.after[prev as usize..] {
          write!(f, "{}", input.interner[c])?;
        }
      }
      write!(f, "\x1b[0m")?;

      return Ok(());
    }

    if let Some(&last) = before.last() {
      for &token in before {
        let token = self.0[token];
        write!(f, "\x1b[31m-{token}")?;
      }
      if !self.0[last].ends_with('\n') {
        writeln!(f)?;
      }
      write!(f, "\x1b[0m")?;
    }
    if let Some(&last) = after.last() {
      for &token in after {
        let token = self.0[token];
        write!(f, "\x1b[32m+{token}")?;
      }
      if !self.0[last].ends_with('\n') {
        writeln!(f)?;
      }
      write!(f, "\x1b[0m")?;
    }
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn bar() {
    use imara_diff::{Algorithm, Diff, InternedInput, UnifiedDiffConfig};

    let before = r#"
fn foo() -> Bar {
  let a = 3;
}
"#;

    let after = r#"
fn foo() -> Bar {
  let b = 3;
}
"#;
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    println!(
      "{}",
      diff.unified_diff(&ColorLinePrinter(&input.interner), UnifiedDiffConfig::default(), &input,)
    );
    panic!();
  }

  #[test]
  fn line_diff_works() {
    let before = r#"
fn foo() -> Bar {
  let a = 3;
}
"#;

    let after = r#"
fn foo() -> Bar {
  let b = 3;
}
"#;

    let diff = line_diff(&Document::from(before), &Document::from(after));
    assert_eq!(diff.changes().collect::<Vec<_>>(), [2..3]);
  }
}
