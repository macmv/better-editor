//! Grapheme cluster segmentation as a composable summary.
//!
//! Whether there is a cluster break at some position depends on the text
//! around it, so the number of clusters in `a + b` is not the number in `a`
//! plus the number in `b`. But the break rules (UAX #29, extended clusters,
//! matching `unicode-segmentation`) are a finite state machine over the text:
//! the state after a character says everything the rules need to know about
//! what came before.
//!
//! A [`GraphemeMap`] is the behavior of that machine over a piece of text: for
//! every state the machine could be in before the text, which state it ends
//! in, and how many clusters begin inside the text. Maps compose, so they can
//! be cached in every tree node.
//!
//! A cluster is counted where it begins: stepping over a character reports
//! whether a break comes before it. The count of a text is the number of
//! characters that begin a cluster, with the first character always beginning
//! one.

use std::{collections::HashMap, sync::OnceLock};

use icu_properties::{
  CodePointMapData, CodePointSetData,
  props::{ExtendedPictographic, GraphemeClusterBreak, IndicConjunctBreak},
};

/// The most states the machine may have after minimization. Maps are stored
/// as fixed arrays of this size.
pub const MAX_STATES: usize = 16;

/// A state of the segmentation machine. [`State::START`] is the state at the
/// start of the text, and also after any line break.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct State(u8);

impl State {
  pub const START: State = State(0);
}

impl Default for State {
  fn default() -> Self { State::START }
}

/// The grapheme break property of a character, with the Indic conjunct and
/// emoji properties folded in the way `unicode-segmentation` does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Cat {
  Any,
  CR,
  Control,
  Extend,
  ExtPict,
  InCbConsonant,
  L,
  LF,
  LV,
  LVT,
  Prepend,
  RI,
  SpacingMark,
  T,
  V,
  ZWJ,
}

/// The Indic_Conjunct_Break property.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Incb {
  None,
  Extend,
  Linker,
  Consonant,
}

const CATS: [Cat; 16] = [
  Cat::Any,
  Cat::CR,
  Cat::Control,
  Cat::Extend,
  Cat::ExtPict,
  Cat::InCbConsonant,
  Cat::L,
  Cat::LF,
  Cat::LV,
  Cat::LVT,
  Cat::Prepend,
  Cat::RI,
  Cat::SpacingMark,
  Cat::T,
  Cat::V,
  Cat::ZWJ,
];

/// The `(Cat, Incb)` combinations the machine distinguishes. Every character
/// maps to one of these (checked against all of Unicode in the tests).
fn classes() -> Vec<(Cat, Incb)> {
  let mut classes = vec![];
  for cat in CATS {
    if cat != Cat::InCbConsonant {
      classes.push((cat, Incb::None));
    }
  }
  classes.push((Cat::Extend, Incb::Extend));
  classes.push((Cat::ZWJ, Incb::Extend));
  classes.push((Cat::Extend, Incb::Linker));
  classes.push((Cat::InCbConsonant, Incb::Consonant));
  classes
}

const NUM_CLASSES: usize = 19;

fn class_index(cat: Cat, incb: Incb) -> Option<u8> {
  // This is the order `classes` lists them in. `Cat`'s discriminants follow
  // `CATS`, and `InCbConsonant` (5) is left out of the plain classes.
  let plain = |cat: Cat| {
    let i = cat as usize;
    if i > Cat::InCbConsonant as usize { i - 1 } else { i }
  };
  match (cat, incb) {
    (Cat::InCbConsonant, Incb::Consonant) => Some(18),
    (Cat::InCbConsonant, _) | (_, Incb::Consonant) => None,
    (Cat::Extend, Incb::Extend) => Some(15),
    (Cat::ZWJ, Incb::Extend) => Some(16),
    (Cat::Extend, Incb::Linker) => Some(17),
    (_, Incb::Extend | Incb::Linker) => None,
    (cat, Incb::None) => Some(plain(cat) as u8),
  }
}

fn categorize(c: char) -> (Cat, Incb) {
  let incb = match CodePointMapData::<IndicConjunctBreak>::new().get(c) {
    IndicConjunctBreak::Consonant => Incb::Consonant,
    IndicConjunctBreak::Extend => Incb::Extend,
    IndicConjunctBreak::Linker => Incb::Linker,
    _ => Incb::None,
  };

  let cat = if incb == Incb::Consonant {
    Cat::InCbConsonant
  } else if CodePointSetData::new::<ExtendedPictographic>().contains(c) {
    Cat::ExtPict
  } else {
    match CodePointMapData::<GraphemeClusterBreak>::new().get(c) {
      GraphemeClusterBreak::Control => Cat::Control,
      GraphemeClusterBreak::CR => Cat::CR,
      GraphemeClusterBreak::LF => Cat::LF,
      GraphemeClusterBreak::Extend => Cat::Extend,
      GraphemeClusterBreak::ZWJ => Cat::ZWJ,
      GraphemeClusterBreak::L => Cat::L,
      GraphemeClusterBreak::V => Cat::V,
      GraphemeClusterBreak::T => Cat::T,
      GraphemeClusterBreak::LV => Cat::LV,
      GraphemeClusterBreak::LVT => Cat::LVT,
      GraphemeClusterBreak::Prepend => Cat::Prepend,
      GraphemeClusterBreak::RegionalIndicator => Cat::RI,
      GraphemeClusterBreak::SpacingMark => Cat::SpacingMark,
      _ => Cat::Any,
    }
  };

  (cat, incb)
}

/// What the pair of properties around a position says about a break there.
enum Pair {
  NotBreak,
  Break,
  /// A break unless preceded by a consonant, then linkers and extenders, with
  /// at least one linker (GB9c).
  IncbConsonant,
  /// A break if preceded by an even number of regional indicators (GB12/13).
  Regional,
  /// A break unless preceded by an emoji, extenders, and a ZWJ (GB11).
  Emoji,
}

fn pair(before: Cat, after: Cat) -> Pair {
  use Cat::*;
  match (before, after) {
    (CR, LF) => Pair::NotBreak,
    (Control | CR | LF, _) => Pair::Break,
    (_, Control | CR | LF) => Pair::Break,
    (L, L | V | LV | LVT) => Pair::NotBreak,
    (LV | V, V | T) => Pair::NotBreak,
    (LVT | T, T) => Pair::NotBreak,
    (_, Extend | ZWJ) => Pair::NotBreak,
    (_, SpacingMark) => Pair::NotBreak,
    (Prepend, _) => Pair::NotBreak,
    (_, InCbConsonant) => Pair::IncbConsonant,
    (ZWJ, ExtPict) => Pair::Emoji,
    (RI, RI) => Pair::Regional,
    _ => Pair::Break,
  }
}

/// A state of the machine before minimization: everything the rules look back
/// at.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Full {
  /// The previous character's category. `None` at the start of the text.
  last:   Option<Cat>,
  /// 0: not in a conjunct. 1: a consonant followed by extenders. 2: the same,
  /// with a linker seen.
  incb:   u8,
  /// 0: nothing. 1: an emoji followed by extenders. 2: the same, then a ZWJ.
  emoji:  u8,
  /// Whether the run of regional indicators ending here has an odd length.
  ri_odd: bool,
}

const FULL_START: Full = Full { last: None, incb: 0, emoji: 0, ri_odd: false };

/// Steps over one character. Returns the new state, and whether the character
/// begins a new cluster.
fn full_step(st: Full, cat: Cat, incb: Incb) -> (Full, bool) {
  let breaks = match st.last {
    None => true,
    Some(before) => match pair(before, cat) {
      Pair::NotBreak => false,
      Pair::Break => true,
      Pair::IncbConsonant => st.incb != 2,
      Pair::Regional => !st.ri_odd,
      Pair::Emoji => st.emoji != 2,
    },
  };

  let next = Full {
    last:   Some(cat),
    incb:   match incb {
      Incb::Consonant => 1,
      Incb::Linker if st.incb != 0 => 2,
      Incb::Linker => 0,
      Incb::Extend => st.incb,
      Incb::None => 0,
    },
    emoji:  match cat {
      Cat::ExtPict => 1,
      Cat::Extend if st.emoji == 1 => 1,
      Cat::ZWJ if st.emoji == 1 => 2,
      _ => 0,
    },
    ri_odd: cat == Cat::RI && !(st.last == Some(Cat::RI) && st.ri_odd),
  };

  (next, breaks)
}

/// The minimized machine.
struct Dfa {
  /// The number of states.
  n:     usize,
  /// `trans[state * NUM_CLASSES + class]` is the next state, and whether the
  /// character begins a cluster.
  trans: Vec<(u8, bool)>,
  /// The class of each ASCII character.
  ascii: [u8; 128],
  /// The state after an ordinary character. From here, every printable ASCII
  /// character begins a cluster and leads back to this state, so runs of them
  /// can be skipped.
  plain: u8,
  /// The state after each ASCII byte that forgets what came before it (every
  /// one but `\r`), whatever the state was.
  forget: [u8; 128],
}

fn dfa() -> &'static Dfa {
  static DFA: OnceLock<Dfa> = OnceLock::new();
  DFA.get_or_init(build_dfa)
}

fn build_dfa() -> Dfa {
  let classes = classes();
  assert_eq!(classes.len(), NUM_CLASSES);

  // Discover every reachable state.
  let mut states = vec![FULL_START];
  let mut index: HashMap<Full, usize> = HashMap::from([(FULL_START, 0)]);
  let mut full_trans: Vec<[(usize, bool); NUM_CLASSES]> = vec![];
  let mut i = 0;
  while i < states.len() {
    let mut row = [(0, false); NUM_CLASSES];
    for (c, &(cat, incb)) in classes.iter().enumerate() {
      let (next, breaks) = full_step(states[i], cat, incb);
      let n = *index.entry(next).or_insert_with(|| {
        states.push(next);
        states.len() - 1
      });
      row[c] = (n, breaks);
    }
    full_trans.push(row);
    i += 1;
  }

  // Merge states that behave identically (Moore's algorithm).
  let mut part = vec![0usize; states.len()];
  let mut count = 1;
  loop {
    let mut ids: HashMap<Vec<(usize, bool)>, usize> = HashMap::new();
    let mut next_part = vec![0; states.len()];
    for s in 0..states.len() {
      let sig: Vec<(usize, bool)> =
        full_trans[s].iter().map(|&(n, breaks)| (part[n], breaks)).collect();
      let len = ids.len();
      next_part[s] = *ids.entry(sig).or_insert(len);
    }
    let next_count = ids.len();
    part = next_part;
    if next_count == count {
      break;
    }
    count = next_count;
  }

  // Number the merged states so the start state is 0.
  let mut renumber = vec![usize::MAX; count];
  let mut n = 0;
  renumber[part[0]] = 0;
  n += 1;
  for s in 0..states.len() {
    if renumber[part[s]] == usize::MAX {
      renumber[part[s]] = n;
      n += 1;
    }
  }
  assert!(n <= MAX_STATES, "the grapheme machine has {n} states; raise MAX_STATES");

  let mut trans = vec![(0u8, false); n * NUM_CLASSES];
  for s in 0..states.len() {
    let from = renumber[part[s]];
    for c in 0..NUM_CLASSES {
      let (next, breaks) = full_trans[s][c];
      trans[from * NUM_CLASSES + c] = (renumber[part[next]] as u8, breaks);
    }
  }

  let mut ascii = [0u8; 128];
  for (b, slot) in ascii.iter_mut().enumerate() {
    let (cat, incb) = categorize(b as u8 as char);
    *slot = class_index(cat, incb).unwrap();
  }

  let plain = trans[ascii[b'a' as usize] as usize].0;
  for b in 0x20..0x7f_u8 {
    assert_eq!(trans[plain as usize * NUM_CLASSES + ascii[b as usize] as usize], (plain, true));
  }

  // Simple ASCII (printable, tab, and newline) always begins a cluster and
  // forgets everything before it, except that the first character can join a
  // preceding prepend character.
  let mut forget = [0u8; 128];
  let simple = |b: u8| (0x20..0x7f).contains(&b) || b == b'\n' || b == b'\t';
  for b in (0..128u8).filter(|&b| simple(b)) {
    let class = ascii[b as usize] as usize;
    forget[b as usize] = trans[class].0;
    for s in 0..n {
      assert_eq!(trans[s * NUM_CLASSES + class].0, forget[b as usize]);
    }
  }
  for a in (0..128u8).filter(|&b| simple(b)) {
    for b in (0..128u8).filter(|&b| simple(b)) {
      let (state, breaks) = trans[forget[a as usize] as usize * NUM_CLASSES + ascii[b as usize] as usize];
      assert_eq!((state, breaks), (forget[b as usize], true));
    }
  }

  Dfa { n, trans, ascii, plain, forget }
}

/// Looking a character up in the Unicode property tables is slow (several
/// binary searches), so classes are cached in blocks of consecutive characters,
/// each filled in the first time something in it is seen.
const BLOCK: usize = 128;
const NUM_BLOCKS: usize = 0x11_0000 / BLOCK;
static BLOCKS: [OnceLock<Box<[u8; BLOCK]>>; NUM_BLOCKS] = [const { OnceLock::new() }; NUM_BLOCKS];

fn class_of(c: char) -> usize {
  let cp = c as usize;
  let block = BLOCKS[cp / BLOCK].get_or_init(|| {
    let base = cp / BLOCK * BLOCK;
    let mut block = Box::new([0u8; BLOCK]);
    for (i, slot) in block.iter_mut().enumerate() {
      if let Some(c) = char::from_u32((base + i) as u32) {
        let (cat, incb) = categorize(c);
        *slot = class_index(cat, incb).expect("character with an unexpected grapheme class");
      }
    }
    block
  });
  block[cp % BLOCK] as usize
}

impl Dfa {
  #[inline]
  fn class(&self, c: char) -> usize {
    if (c as u32) < 128 { self.ascii[c as usize] as usize } else { class_of(c) }
  }

  #[inline]
  fn step(&self, state: u8, class: usize) -> (u8, bool) {
    self.trans[state as usize * NUM_CLASSES + class]
  }
}

/// Steps the machine over one character. Returns the new state, and whether
/// the character begins a new cluster.
#[inline]
pub fn step(state: State, c: char) -> (State, bool) {
  let dfa = dfa();
  let (next, breaks) = dfa.step(state.0, dfa.class(c));
  (State(next), breaks)
}

/// Runs the machine over `text`. Returns the new state, and the number of
/// clusters that begin in `text`.
pub fn advance(mut state: State, text: &str) -> (State, usize) {
  let dfa = dfa();
  let mut count = 0;
  let mut rest = text;
  while let Some(c) = rest.chars().next() {
    if state.0 == dfa.plain && (0x20..0x7f).contains(&(c as u32)) {
      // Every printable ASCII character begins a cluster here, and leaves the
      // machine where it was. Skip a whole run of them.
      let run = rest.bytes().take_while(|b| (0x20..0x7f).contains(b)).count();
      count += run;
      rest = &rest[run..];
      continue;
    }

    let (next, breaks) = dfa.step(state.0, dfa.class(c));
    state = State(next);
    count += breaks as usize;
    rest = &rest[c.len_utf8()..];
  }
  (state, count)
}

/// The behavior of the machine over some text: for each starting state, the
/// ending state and the number of clusters that begin in the text. Maps
/// compose.
///
/// Counts for different starting states differ by only a little (whether the
/// first character joins the cluster before it), so only the smallest count is
/// stored in full.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GraphemeMap {
  base:  usize,
  end:   [u8; MAX_STATES],
  delta: [u8; MAX_STATES],
}

impl Default for GraphemeMap {
  /// The map of the empty text.
  fn default() -> Self { GraphemeMap::identity() }
}

impl GraphemeMap {
  fn identity() -> GraphemeMap {
    let mut end = [0; MAX_STATES];
    for (s, e) in end.iter_mut().enumerate() {
      *e = s as u8;
    }
    GraphemeMap { base: 0, end, delta: [0; MAX_STATES] }
  }

  fn from_counts(end: [u8; MAX_STATES], counts: [usize; MAX_STATES]) -> GraphemeMap {
    let n = dfa().n;
    let base = counts[..n].iter().copied().min().unwrap_or(0);
    let mut delta = [0; MAX_STATES];
    for s in 0..n {
      let d = counts[s] - base;
      assert!(d <= u8::MAX as usize, "grapheme counts diverge by {d}");
      delta[s] = d as u8;
    }
    GraphemeMap { base, end, delta }
  }

  /// The map of `text`.
  pub fn of(text: &str) -> GraphemeMap {
    let dfa = dfa();
    let n = dfa.n;

    let mut chars = text.chars();
    let Some(first) = chars.next() else { return GraphemeMap::identity() };

    // Run all starting states side by side. They quickly end up in the same
    // state (a cluster boundary forgets the past), after which one run
    // follows them all. Starting states that have merged are tracked as a
    // group, plus an offset for how many more clusters each has counted.
    // After the first character, most starting states have already merged.
    let mut group_of = [0usize; MAX_STATES];
    let mut offset = [0usize; MAX_STATES];
    // (state, count) of each group.
    let mut groups: Vec<(u8, usize)> = Vec::with_capacity(n);
    let class = dfa.class(first);
    for s in 0..n {
      let (state, breaks) = dfa.step(s as u8, class);
      let group = match groups.iter().position(|g| g.0 == state) {
        Some(group) => group,
        None => {
          groups.push((state, 0));
          groups.len() - 1
        }
      };
      group_of[s] = group;
      offset[s] = breaks as usize;
    }

    while groups.len() > 1 {
      let Some(c) = chars.next() else { break };
      let class = dfa.class(c);
      for g in groups.iter_mut() {
        let (next, breaks) = dfa.step(g.0, class);
        *g = (next, g.1 + breaks as usize);
      }

      // Merge groups that landed in the same state.
      let mut i = 0;
      while i < groups.len() {
        let mut j = i + 1;
        while j < groups.len() {
          if groups[j].0 == groups[i].0 {
            // Fold group `j` into `i`. The counts can differ either way, so
            // keep the offsets as wrapping differences.
            let diff = groups[j].1.wrapping_sub(groups[i].1);
            let last = groups.len() - 1;
            for s in 0..n {
              if group_of[s] == j {
                group_of[s] = i;
                offset[s] = offset[s].wrapping_add(diff);
              }
            }
            // Move the last group into the hole.
            groups.swap_remove(j);
            if j != last {
              for s in 0..n {
                if group_of[s] == last {
                  group_of[s] = j;
                }
              }
            }
          } else {
            j += 1;
          }
        }
        i += 1;
      }
    }

    // Either one group is left or the text ran out. Run whatever is left; the
    // groups can still be in different states, so each follows it on its own.
    let rest = chars.as_str();
    if !rest.is_empty() {
      for g in groups.iter_mut() {
        let (state, count) = advance(State(g.0), rest);
        *g = (state.0, g.1 + count);
      }
    }

    let mut end = [0u8; MAX_STATES];
    let mut counts = [0usize; MAX_STATES];
    for (s, e) in end.iter_mut().enumerate() {
      *e = s as u8;
    }
    for s in 0..n {
      let (state, count) = groups[group_of[s]];
      end[s] = state;
      counts[s] = count.wrapping_add(offset[s]);
    }
    GraphemeMap::from_counts(end, counts)
  }

  /// The map of non-empty text made only of printable ASCII, tabs, and
  /// newlines. Every character begins a cluster, except that the first can join
  /// onto the text before it.
  pub(crate) fn of_simple_ascii(text: &str) -> GraphemeMap {
    let dfa = dfa();
    let bytes = text.as_bytes();
    let (first, last) = (bytes[0], bytes[bytes.len() - 1]);

    let mut end = [0u8; MAX_STATES];
    let mut counts = [0usize; MAX_STATES];
    for (s, e) in end.iter_mut().enumerate() {
      *e = s as u8;
    }
    for s in 0..dfa.n {
      let (state, breaks) = dfa.step(s as u8, dfa.ascii[first as usize] as usize);
      if bytes.len() == 1 {
        end[s] = state;
        counts[s] = breaks as usize;
      } else {
        end[s] = dfa.forget[last as usize];
        counts[s] = breaks as usize + bytes.len() - 1;
      }
    }
    GraphemeMap::from_counts(end, counts)
  }

  /// Applies the map to a starting state. Returns the ending state, and the
  /// number of clusters that begin in the text.
  #[inline]
  pub fn apply(&self, state: State) -> (State, usize) {
    let s = state.0 as usize;
    (State(self.end[s]), self.base + self.delta[s] as usize)
  }

  /// The map of this text followed by `other`'s.
  pub fn then(&self, other: &GraphemeMap) -> GraphemeMap {
    let n = dfa().n;
    let mut end = [0u8; MAX_STATES];
    let mut counts = [0usize; MAX_STATES];
    for (s, e) in end.iter_mut().enumerate() {
      *e = s as u8;
    }
    for s in 0..n {
      let (mid, a) = self.apply(State(s as u8));
      let (last, b) = other.apply(mid);
      end[s] = last.0;
      counts[s] = a + b;
    }
    GraphemeMap::from_counts(end, counts)
  }
}

#[cfg(test)]
mod tests {
  use unicode_segmentation::UnicodeSegmentation;

  use super::*;

  /// A deterministic pseudo-random generator.
  pub(crate) struct Rng(pub u64);

  impl Rng {
    pub fn next(&mut self, n: usize) -> usize {
      self.0 ^= self.0 << 13;
      self.0 ^= self.0 >> 7;
      self.0 ^= self.0 << 17;
      (self.0 % n as u64) as usize
    }
  }

  /// Characters that exercise each rule.
  pub(crate) const INTERESTING: &[char] = &[
    'a',
    'b',
    ' ',
    '\n',
    '\r',
    '\t',
    '\u{7f}',
    '\u{e9}',
    'e',
    '\u{301}',   // combining acute
    '\u{200d}',  // ZWJ
    '\u{1f1e6}', // regional indicators
    '\u{1f1e7}',
    '\u{1f1e8}',
    '\u{1f468}', // man
    '\u{1f469}', // woman
    '\u{1f3fb}', // emoji modifier
    '\u{2764}',  // heart
    '\u{fe0f}',  // variation selector
    '\u{1100}',  // hangul L
    '\u{1161}',  // hangul V
    '\u{11a8}',  // hangul T
    '\u{ac00}',  // hangul LV
    '\u{ac01}',  // hangul LVT
    '\u{600}',   // prepend
    '\u{903}',   // spacing mark
    '\u{915}',   // devanagari ka (consonant)
    '\u{937}',   // devanagari ssa
    '\u{94d}',   // virama (linker)
    '\u{93c}',   // nukta (extend)
    '\u{9a4}',   // bengali ta
    '\u{9cd}',   // bengali virama
    '\u{200b}',  // zero width space
    '\u{ad}',    // soft hyphen
    '\u{1f600}', // grinning face
    '\u{a9}',
    '\u{3042}',
    '\u{20e3}', // keycap
  ];

  pub(crate) fn random_string(rng: &mut Rng, max: usize) -> String {
    let len = rng.next(max + 1);
    (0..len)
      .map(|_| match rng.next(10) {
        0 => char::from_u32(rng.next(0x3000) as u32).unwrap_or('?'),
        1 => char::from_u32(0x1f000 + rng.next(0x700) as u32).unwrap_or('?'),
        _ => INTERESTING[rng.next(INTERESTING.len())],
      })
      .collect()
  }

  fn count(text: &str) -> usize { advance(State::START, text).1 }

  fn oracle(text: &str) -> usize { text.graphemes(true).count() }

  #[test]
  fn simple_ascii_closed_form_matches_the_machine() {
    let simple: Vec<char> = (0x20..0x7f).chain([b'\n', b'\t']).map(|b| b as u8 as char).collect();
    let mut rng = Rng(0x5eed);
    for _ in 0..5_000 {
      let len = 1 + rng.next(12);
      let text: String = (0..len).map(|_| simple[rng.next(simple.len())]).collect();
      assert_eq!(GraphemeMap::of_simple_ascii(&text), GraphemeMap::of(&text), "{text:?}");
    }
  }

  #[test]
  fn class_indexes_match_the_class_list() {
    for (i, &(cat, incb)) in classes().iter().enumerate() {
      assert_eq!(class_index(cat, incb), Some(i as u8), "{cat:?}/{incb:?}");
    }
  }

  #[test]
  fn state_count() {
    let n = dfa().n;
    println!("grapheme machine has {n} states");
    assert!(n <= MAX_STATES);
  }

  #[test]
  fn start_state_forgets_nothing() {
    // The start state behaves like the state after any line break.
    for text in ["", "a", "\u{301}", "\u{1f1e6}\u{1f1e7}", "\u{200d}\u{1f600}"] {
      let after_lf = advance(State::START, "\n").0;
      assert_eq!(advance(after_lf, text).1, count(text), "{text:?}");
      let after_cr = advance(State::START, "\r").0;
      assert_eq!(advance(after_cr, text).1, count(text), "{text:?}");
    }
  }

  #[test]
  fn matches_unicode_segmentation() {
    let mut rng = Rng(0x2545f4914f6cdd1d);
    for _ in 0..20_000 {
      let s = random_string(&mut rng, 24);
      assert_eq!(count(&s), oracle(&s), "{s:?}");
    }
  }

  #[test]
  fn maps_compose_across_any_split() {
    let mut rng = Rng(0x9e3779b97f4a7c15);
    for _ in 0..5_000 {
      let s = random_string(&mut rng, 30);
      let whole = GraphemeMap::of(&s);
      assert_eq!(whole.apply(State::START).1, oracle(&s), "{s:?}");

      // Split into random pieces and compose.
      let mut composed = GraphemeMap::default();
      let mut rest = s.as_str();
      while !rest.is_empty() {
        let mut cut = rng.next(rest.len() + 1);
        while !rest.is_char_boundary(cut) {
          cut -= 1;
        }
        composed = composed.then(&GraphemeMap::of(&rest[..cut]));
        rest = &rest[cut..];
      }
      assert_eq!(composed, whole, "{s:?}");
    }
  }

  #[test]
  fn map_matches_stepping_from_every_state() {
    let mut rng = Rng(0xdeadbeefcafef00d);
    let n = dfa().n;
    for _ in 0..3_000 {
      let s = random_string(&mut rng, 20);
      let map = GraphemeMap::of(&s);
      for state in 0..n {
        let expected = advance(State(state as u8), &s);
        assert_eq!(map.apply(State(state as u8)), expected, "{s:?} from {state}");
      }
    }
  }

  #[test]
  fn long_runs() {
    // Regional indicators pair up, so parity matters over a long run.
    for n in [1, 2, 3, 10, 11, 1000, 1001] {
      let s = "\u{1f1e6}".repeat(n);
      assert_eq!(count(&s), oracle(&s));
      assert_eq!(GraphemeMap::of(&s).apply(State::START).1, n.div_ceil(2));
    }
    // A long emoji sequence is one cluster.
    let mut s = String::from("\u{1f468}");
    for _ in 0..500 {
      s.push_str("\u{200d}\u{1f469}");
    }
    assert_eq!(count(&s), 1);
    assert_eq!(oracle(&s), 1);
    // So is a base with a long run of combining marks.
    let s = format!("e{}", "\u{301}".repeat(5000));
    assert_eq!(count(&s), 1);
  }

  /// Counts for different starting states must stay within what a `u8` can
  /// hold. Following every pair of starting states through the machine, the
  /// difference in their counts is bounded as long as no cycle changes it.
  #[test]
  fn counts_stay_close() {
    let dfa = dfa();
    let n = dfa.n;

    // best[(a, b)] is the largest count(a) - count(b) over all texts reaching
    // the pair. Relax until stable; if it never stabilizes, a cycle grows it.
    let mut best: HashMap<(u8, u8), i64> = HashMap::new();
    for a in 0..n {
      for b in 0..n {
        best.insert((a as u8, b as u8), 0);
      }
    }
    for round in 0..=n * n + 1 {
      let mut changed = false;
      let snapshot: Vec<_> = best.iter().map(|(&k, &v)| (k, v)).collect();
      for ((a, b), v) in snapshot {
        for class in 0..NUM_CLASSES {
          let (na, ba) = dfa.step(a, class);
          let (nb, bb) = dfa.step(b, class);
          let w = v + ba as i64 - bb as i64;
          let slot = best.entry((na, nb)).or_insert(i64::MIN);
          if w > *slot {
            *slot = w;
            changed = true;
          }
        }
      }
      if !changed {
        let max = best.values().copied().max().unwrap();
        println!("counts differ by at most {max}");
        assert!(max <= u8::MAX as i64);
        return;
      }
      assert!(round <= n * n, "count differences are unbounded");
    }
    panic!("count differences are unbounded");
  }

  #[test]
  fn every_character_has_a_class() {
    for c in (0..=0x10ffff).filter_map(char::from_u32) {
      let (cat, incb) = categorize(c);
      assert!(class_index(cat, incb).is_some(), "{c:?} is {cat:?}/{incb:?}");
    }
  }

  /// Checks our character classes against `unicode-segmentation`'s own, by
  /// comparing how the two segment each character in many contexts.
  #[test]
  fn character_classes_match_unicode_segmentation() {
    let before: &[&str] = &[
      "",
      "a",
      "\r",
      "\n",
      "\u{1f1e6}",
      "\u{1f1e6}\u{1f1e7}",
      "\u{1f468}\u{200d}",
      "\u{1f468}",
      "\u{1f468}\u{1f3fb}\u{200d}",
      "\u{600}",
      "\u{915}\u{94d}",
      "\u{915}\u{93c}\u{94d}",
      "\u{915}",
      "\u{1100}",
      "\u{1161}",
      "\u{ac00}",
      "\u{ac01}",
      "\u{11a8}",
    ];
    let after: &[&str] =
      &["", "a", "\u{1f1e6}", "\u{1f468}", "\u{915}", "\u{301}", "\u{1161}", "\u{11a8}"];

    let check = |c: char| {
      for b in before {
        for a in after {
          let s = format!("{b}{c}{a}");
          assert_eq!(count(&s), oracle(&s), "{s:?} (U+{:04X})", c as u32);
        }
      }
    };

    for c in (0..=0x10ffff).filter_map(char::from_u32) {
      let (cat, incb) = categorize(c);
      // Test every character with any property, and a sample of the rest.
      if cat != Cat::Any || incb != Incb::None || (c as u32) % 61 == 0 {
        check(c);
      }
    }
  }
}
