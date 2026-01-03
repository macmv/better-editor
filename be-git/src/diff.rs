#![allow(dead_code)]

use be_doc::Document;
use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};
use siphasher::sip::SipHasher;

use std::{
  fmt,
  hash::{Hash, Hasher},
  ops::Range,
};

struct ColorLinePrinter<'a>(&'a imara_diff::Interner<RopeSliceHash<'a>>);
struct CharTokens<'a>(RopeSliceHash<'a>);

pub struct LineDiff {
  diff: Diff,
}

pub struct LineHunk {
  pub range: Range<usize>,
}

pub fn line_diff(before: &Document, after: &Document) -> LineDiff {
  let input = InternedInput::new(DocLines(before), DocLines(after));
  let mut diff = Diff::compute(Algorithm::Histogram, &input);
  diff.postprocess_no_heuristic(&input);

  LineDiff { diff }
}

impl LineDiff {
  pub fn changes(&'_ self) -> impl Iterator<Item = LineHunk> + use<'_> {
    self.diff.hunks().map(|hunk| LineHunk::new(&hunk))
  }
}

impl LineHunk {
  pub fn new(hunk: &imara_diff::Hunk) -> Self {
    LineHunk { range: hunk.after.start as usize..hunk.after.end as usize }
  }
}

struct DocLines<'a>(&'a Document);
#[derive(PartialEq, Eq)]
struct RopeSliceHash<'a>(be_doc::crop::RopeSlice<'a>);

impl Hash for RopeSliceHash<'_> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    // The hasher used by `imara_diff` is sequence dependent (it's foldhash). So, we
    // use siphasher, which handles different chunk boundaries correctly.
    let mut hasher = SipHasher::new();
    for chunk in self.0.chunks() {
      hasher.write(chunk.as_bytes());
    }
    state.write_u64(hasher.finish());
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

impl<'a> TokenSource for CharTokens<'a> {
  type Token = char;
  type Tokenizer = be_doc::crop::iter::Chars<'a>;

  fn tokenize(&self) -> Self::Tokenizer { self.0.0.chars() }

  fn estimate_tokens(&self) -> u32 { self.0.0.byte_len() as u32 }
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
    write!(f, " {}", &self.0[token].0)?;
    if !&self.0[token].0.chunks().last().is_some_and(|c| c.ends_with('\n')) {
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
      /*
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
      */

      return Ok(());
    }

    if let Some(&last) = before.last() {
      for &token in before {
        let token = &self.0[token];
        write!(f, "\x1b[31m-{}", token.0)?;
      }
      if !self.0[last].0.chunks().last().is_some_and(|c| c.ends_with('\n')) {
        writeln!(f)?;
      }
      write!(f, "\x1b[0m")?;
    }
    if let Some(&last) = after.last() {
      for &token in after {
        let token = &self.0[token];
        write!(f, "\x1b[32m+{}", token.0)?;
      }
      if !self.0[last].0.chunks().last().is_some_and(|c| c.ends_with('\n')) {
        writeln!(f)?;
      }
      write!(f, "\x1b[0m")?;
    }
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use imara_diff::UnifiedDiffConfig;

  use super::*;

  #[test]
  fn bar() {
    use imara_diff::{Algorithm, Diff, InternedInput};

    let before = Document::from(
      r#"
fn foo() -> Bar {
  let a = 3;
}
"#,
    );

    let after = Document::from(
      r#"
fn foo() -> Bar {
  let b = 3;
}
"#,
    );
    let input = InternedInput::new(DocLines(&before), DocLines(&after));
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_no_heuristic(&input);

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

    let before = Document::from(before);
    let after = Document::from(after);
    let diff = line_diff(&before, &after);
    assert_eq!(diff.changes().collect::<Vec<_>>()[0].range, 2..3);
  }

  #[test]
  fn line_diff_changes() {
    let before = r#"
fn foo() -> Bar {
  let aaa = 3;
  let ccc = 3;
}
"#;

    let after = r#"
fn foo() -> Bar {
  let aba = 3;
  let b = 3;
  let cbc = 3;
}
"#;

    let before = Document::from(before);
    let after = Document::from(after);
    let diff = line_diff(&before, &after);
    assert_eq!(diff.changes().collect::<Vec<_>>()[0].range, 2..5);
  }
}
