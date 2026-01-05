#![allow(dead_code)]

use be_doc::{Document, crop::RopeSlice};
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

pub struct LineDiffSimilarity {
  hunks: Vec<LineHunkSimilarity>,
}

pub struct LineHunk {
  pub range: Range<usize>,
}

pub struct LineHunkSimilarity {
  pub before:  Range<usize>,
  pub after:   Range<usize>,
  pub changes: Vec<Change>,
}

pub fn line_diff(before: &Document, after: &Document) -> LineDiff {
  line_diff_inner(before, after).0
}

fn line_diff_inner<'a>(
  before: &'a Document,
  after: &'a Document,
) -> (LineDiff, InternedInput<RopeSliceHash<'a>>) {
  let input = InternedInput::new(DocLines(before), DocLines(after));
  let mut diff = Diff::compute(Algorithm::Histogram, &input);
  diff.postprocess_no_heuristic(&input);

  (LineDiff { diff }, input)
}

pub fn line_diff_similarity<'a>(before: &'a Document, after: &'a Document) -> LineDiffSimilarity {
  let input = InternedInput::new(DocLines(before), DocLines(after));
  let mut diff = Diff::compute(Algorithm::Histogram, &input);
  diff.postprocess_no_heuristic(&input);

  let mut hunks = vec![];

  for hunk in diff.hunks() {
    let before = hunk.before.start as usize..hunk.before.end as usize;
    let after = hunk.after.start as usize..hunk.after.end as usize;

    let changes = similarity_diff(&input, before.clone(), after.clone());

    hunks.push(LineHunkSimilarity { before, after, changes });
  }

  LineDiffSimilarity { hunks }
}

impl LineDiff {
  pub fn changes(&'_ self) -> impl Iterator<Item = LineHunk> + use<'_> {
    self.diff.hunks().map(|hunk| LineHunk::new(&hunk))
  }
}

impl LineDiffSimilarity {
  pub fn changes(&'_ self) -> impl Iterator<Item = &LineHunkSimilarity> + use<'_> {
    self.hunks.iter()
  }
}

impl LineHunk {
  pub fn new(hunk: &imara_diff::Hunk) -> Self {
    LineHunk { range: hunk.after.start as usize..hunk.after.end as usize }
  }
}

struct DocLines<'a>(&'a Document);
#[derive(Clone, Copy, PartialEq, Eq)]
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
    be_doc::crop::iter::RawLines<'a>,
    fn(be_doc::crop::RopeSlice<'a>) -> RopeSliceHash<'a>,
  >;

  fn tokenize(&self) -> Self::Tokenizer { self.0.rope.raw_lines().map(|l| RopeSliceHash(l)) }

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
      let before_slice = self.0[before[0]];
      let after_slice = self.0[after[0]];

      let input = InternedInput::new(CharTokens(before_slice), CharTokens(after_slice));
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
      if prev < input.after.len() {
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
      if prev < input.after.len() {
        for &c in &input.after[prev as usize..] {
          write!(f, "{}", input.interner[c])?;
        }
      }
      write!(f, "\x1b[0m")?;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ChangeKind {
  Modify,
  Add,
  Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
  before_start: usize,
  after_start:  usize,
  length:       usize,
  kind:         ChangeKind,
}

impl Change {
  pub fn before(&self) -> Range<usize> {
    match self.kind {
      ChangeKind::Modify => self.before_start..(self.before_start + self.length),
      ChangeKind::Add => self.before_start..self.before_start,
      ChangeKind::Remove => self.before_start..(self.before_start + self.length),
    }
  }

  pub fn after(&self) -> Range<usize> {
    match self.kind {
      ChangeKind::Modify => self.after_start..(self.after_start + self.length),
      ChangeKind::Add => self.after_start..(self.after_start + self.length),
      ChangeKind::Remove => self.after_start..self.after_start,
    }
  }
}

fn similarity_diff<'a>(
  input: &InternedInput<RopeSliceHash<'a>>,
  before: Range<usize>,
  after: Range<usize>,
) -> Vec<Change> {
  // --- Tunables ---
  const COST_DEL: f32 = 1.0;
  const COST_INS: f32 = 1.0;
  const SIM_THRESHOLD: f32 = 0.30; // don't match "garbage"
  const INFINITY: f32 = 1.0e30;
  const EPSILON: f32 = 1.0e-6;

  // Precomputed similarities
  let mut sim = vec![0.0; before.len() * after.len()];
  for i in 0..before.len() {
    for j in 0..after.len() {
      sim[i * after.len() + j] = line_similarity(
        input.interner[input.before[before.start + i]].0,
        input.interner[input.after[after.start + j]].0,
      );
    }
  }

  // DP tables: costs and backpointers.
  macro_rules! idx {
    ($i:expr, $j:expr) => {{ $i * (after.len() + 1) + $j }};
  }
  let mut dp = vec![INFINITY; (before.len() + 1) * (after.len() + 1)];
  let mut back = vec![ChangeKind::Modify; (before.len() + 1) * (after.len() + 1)];

  dp[idx!(0, 0)] = 0.0;
  for i in 1..=before.len() {
    dp[idx!(i, 0)] = dp[idx!(i - 1, 0)] + COST_DEL;
    back[idx!(i, 0)] = ChangeKind::Remove;
  }
  for j in 1..=after.len() {
    dp[idx!(0, j)] = dp[idx!(0, j - 1)] + COST_INS;
    back[idx!(0, j)] = ChangeKind::Add;
  }

  // Fill DP with deterministic tie-breaks:
  // 1) lower cost
  // 2) prefer Sub over Del over Ins (but Sub is only allowed if sim >= threshold)
  // 3) if Sub ties, prefer higher sim
  for i in 1..=before.len() {
    for j in 1..=after.len() {
      let del_cost = dp[idx!(i - 1, j)] + COST_DEL;
      let ins_cost = dp[idx!(i, j - 1)] + COST_INS;

      let s = sim[(i - 1) * after.len() + (j - 1)];
      let sub_cost = if s >= SIM_THRESHOLD { dp[idx!(i - 1, j - 1)] + (1.0 - s) } else { INFINITY };

      // Choose best with stable tie-breaking.
      let mut best_cost = sub_cost;
      let mut best_step = ChangeKind::Modify;

      if del_cost + EPSILON < best_cost
        || ((del_cost - best_cost).abs() <= EPSILON && ChangeKind::Remove < best_step)
      {
        best_cost = del_cost;
        best_step = ChangeKind::Remove;
      }

      if ins_cost + EPSILON < best_cost
        || ((ins_cost - best_cost).abs() <= EPSILON && ChangeKind::Add < best_step)
      {
        best_cost = ins_cost;
        best_step = ChangeKind::Add;
      }

      dp[idx!(i, j)] = best_cost;
      back[idx!(i, j)] = best_step;
    }
  }

  let mut changes: Vec<Change> = Vec::with_capacity(before.len() + after.len());
  let mut i = before.len();
  let mut j = after.len();
  while i > 0 || j > 0 {
    let kind = back[idx!(i, j)];
    match kind {
      ChangeKind::Modify => {
        i -= 1;
        j -= 1;
      }
      ChangeKind::Add => j -= 1,
      ChangeKind::Remove => i -= 1,
    };

    if let Some(last_change) = changes.last_mut() {
      if last_change.kind == kind
        && last_change.kind != ChangeKind::Remove
        && last_change.after_start - after.start == j + 1
      {
        last_change.length += 1;
        continue;
      }
    }

    changes.push(Change {
      after_start: after.start + j,
      before_start: before.start + i,
      length: 1,
      kind,
    });
  }

  changes.reverse();
  changes
}

fn line_similarity<'a>(a: RopeSlice<'a>, b: RopeSlice<'a>) -> f32 {
  if a == b {
    return 1.0;
  } else if a.is_empty() && b.is_empty() {
    return 1.0;
  }

  let dist = levenshtein_distance(a, b) as f32;
  let max_len = (a.chars().count().max(b.chars().count())) as f32;
  let sim = 1.0 - (dist / max_len as f32);
  sim.clamp(0.0, 1.0)
}

pub fn levenshtein_distance<'a>(mut a: RopeSlice<'a>, mut b: RopeSlice<'a>) -> usize {
  if a.is_empty() {
    return b.chars().count();
  } else if b.is_empty() {
    return a.chars().count();
  }

  let len_a = a.chars().count();
  let mut len_b = b.chars().count();
  if len_a < len_b {
    std::mem::swap(&mut a, &mut b);
    len_b = len_a;
  }

  let mut pre;
  let mut tmp;
  let mut curr = vec![0; len_b + 1];
  for i in 1..=len_b {
    curr[i] = i;
  }

  for (i, ca) in a.chars().enumerate() {
    pre = curr[0];
    curr[0] = i + 1;
    for (j, cb) in b.chars().enumerate() {
      tmp = curr[j + 1];
      curr[j + 1] = std::cmp::min(
        // deletion
        tmp + 1,
        std::cmp::min(
          // insertion
          curr[j] + 1,
          // match or substitution
          pre + if ca == cb { 0 } else { 1 },
        ),
      );
      pre = tmp;
    }
  }
  curr[len_b]
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
  let ccc = 3;
}
"#;

    let after = r#"
fn foo() -> Bar {
  let aba = 3;
  let b = 3;
  let cbc = 3;
  let cbc = 3;
}
"#;

    let before = Document::from(before);
    let after = Document::from(after);
    let diff = line_diff_similarity(&before, &after);

    assert_eq!(diff.hunks[0].after, 2..6);

    // modify 1 line
    assert_eq!(diff.hunks[0].changes[0].before(), 2..3);
    assert_eq!(diff.hunks[0].changes[0].after(), 2..3);

    // add 1 line
    assert_eq!(diff.hunks[0].changes[1].before(), 3..3);
    assert_eq!(diff.hunks[0].changes[1].after(), 3..4);

    // modify 2 lines
    assert_eq!(diff.hunks[0].changes[2].before(), 4..6);
    assert_eq!(diff.hunks[0].changes[2].after(), 5..7);
  }
}
