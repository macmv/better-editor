#![allow(dead_code)]

use std::num::NonZero;

use be_input::{ActionError, ChangeDirection, Direction, Key, KeyStroke, Mode, Navigation};

use crate::{
  grammar,
  keybind::{KeyMap, Keybinds},
};

pub enum Action {
  SetMode { mode: Mode, delta: i32 },
  OpenSearch,
  Append { after: bool },
  Edit { edit: Edit },
  Navigate { nav: Navigation },
  Autocomplete,
  MoveCompletion { next: bool },
  Yank,
  ClipboardCopy,
  ClipboardPaste,
}

#[derive(Debug, Clone, Copy)]
pub enum Move {
  Single(Direction),

  NextWord,
  EndWord,
  PrevWord,
  Char(ChangeDirection),

  LineStart,
  LineStartOfText,
  LineEnd,
  MatchingBracket,

  FileStart,
  FileEnd,

  Result(ChangeDirection),
  Change(ChangeDirection),
  Diagnostic(ChangeDirection),

  GotoDefinition,
  GotoReference,
  BackDefinition,
}

#[derive(Debug)]
pub enum Edit {
  Replace,
  Delete,
  Cut,
  DeleteLine,
  CutLine,
  DeleteRestOfLine,
  Paste { after: bool },
  Backspace,
  Undo,
  Redo,
  SwitchCase,
}

struct KeyParser<'a> {
  keybinds:  &'a Keybinds,
  remaining: &'a [KeyStroke],
}

impl Keybinds {
  pub fn parse(&self, mode: Mode, input: &[KeyStroke]) -> Result<be_input::Action, ActionError> {
    use be_input::*;

    let mut count = 0;

    macro_rules! e {
      ($($e:tt)*) => {
        Ok(Action::Edit { count: NonZero::new(count), e: Edit::$($e)* })
      };
    }
    macro_rules! m {
      ($($e:tt)*) => {
        Ok(Action::Move { count: NonZero::new(count), m: Move::$($e)* })
      };
    }

    let mut parser = KeyParser { keybinds: self, remaining: input };

    if mode == Mode::Normal {
      while let Some(Key::Char(c @ '0'..='9')) = parser.remaining.get(0).map(|k| k.key) {
        // Counts cannot start with 0 (as 0 is start-of-line).
        if c == '0' && count == 0 {
          break;
        }

        count *= 10;
        count += u32::from(c) - u32::from('0');
        parser.remaining = &parser.remaining[1..];
      }
    }

    let key = parser.remaining.get(0).ok_or(ActionError::Incomplete)?;
    return match (mode, key.key) {
      (_, Key::Char('w')) if key.control => {
        match parser.remaining.get(1).ok_or(ActionError::Incomplete)?.key {
          Key::Char('\\') => Ok(Action::Navigate { nav: Navigation::OpenWorkspace }),
          Key::Char('h') => Ok(Action::Navigate { nav: Navigation::Direction(Direction::Left) }),
          Key::Char('j') => Ok(Action::Navigate { nav: Navigation::Direction(Direction::Down) }),
          Key::Char('k') => Ok(Action::Navigate { nav: Navigation::Direction(Direction::Up) }),
          Key::Char('l') => Ok(Action::Navigate { nav: Navigation::Direction(Direction::Right) }),
          Key::Char('v') => Ok(Action::Navigate { nav: Navigation::Split(Axis::Vertical) }),
          Key::Char('f') => Ok(Action::Navigate { nav: Navigation::Split(Axis::Horizontal) }),
          Key::Char(c @ '0'..='9') => Ok(Action::Navigate { nav: Navigation::Tab(c as u8 - b'0') }),
          _ => Err(ActionError::Unrecognized),
        }
      }

      (Mode::Normal, Key::Char(' ')) if !key.control => {
        match parser.remaining.get(1).ok_or(ActionError::Incomplete)?.key {
          Key::Char('s') => Ok(Action::Navigate { nav: Navigation::OpenSearch }),
          _ => Err(ActionError::Unrecognized),
        }
      }

      #[cfg(not(target_os = "macos"))]
      (Mode::Insert, Key::Char('C')) if key.control => Ok(Action::ClipboardCopy),
      #[cfg(not(target_os = "macos"))]
      (Mode::Insert, Key::Char('V')) if key.control => Ok(Action::ClipboardPaste),
      #[cfg(target_os = "macos")]
      (Mode::Insert, Key::Char('c')) if key.sup => Ok(Action::ClipboardCopy),
      #[cfg(target_os = "macos")]
      (Mode::Insert, Key::Char('v')) if key.sup => Ok(Action::ClipboardPaste),

      (Mode::Insert, Key::Char(' ')) if key.control => Ok(Action::Autocomplete),
      (Mode::Insert, Key::Char('p')) if key.control => Ok(Action::MoveCompletion { next: false }),
      (Mode::Insert, Key::Char('n')) if key.control => Ok(Action::MoveCompletion { next: true }),
      (Mode::Insert, Key::Char(c)) if key.control => Ok(Action::Control { char: c }),
      (Mode::Insert, Key::Tab) => Ok(Action::Tab { shift: key.shift }),

      (Mode::Insert | Mode::Command, Key::Char(c)) => e!(Insert(c)),
      (Mode::Insert | Mode::Command, Key::Backspace) => e!(Backspace),
      (Mode::Insert | Mode::Command, Key::Escape) => {
        Ok(Action::SetMode { mode: Mode::Normal, delta: -1 })
      }
      (Mode::Insert | Mode::Command, Key::ArrowUp) => m!(Single(Direction::Up)),
      (Mode::Insert | Mode::Command, Key::ArrowDown) => m!(Single(Direction::Down)),
      (Mode::Insert | Mode::Command, Key::ArrowLeft) => m!(Single(Direction::Left)),
      (Mode::Insert | Mode::Command, Key::ArrowRight) => m!(Single(Direction::Right)),

      (Mode::Visual(_), Key::Escape) => Ok(Action::SetMode { mode: Mode::Normal, delta: 0 }),
      (Mode::Replace, Key::Escape) => Ok(Action::SetMode { mode: Mode::Normal, delta: -1 }),

      // === edits ===
      (Mode::Normal, Key::Char('r')) if !key.control => {
        match parser.remaining.get(1).ok_or(ActionError::Incomplete)?.key {
          Key::Char(c) => e!(Replace(c)),
          _ => Err(ActionError::Unrecognized),
        }
      }
      (Mode::Normal, Key::Char('x')) => e!(Delete(Move::Single(Direction::Right))),
      (Mode::Normal, Key::Char('d')) => {
        match parser.remaining.get(1).ok_or(ActionError::Incomplete)? {
          KeyStroke { key: Key::Char('d'), .. } => e!(DeleteLine),
          _ => {
            parser.skip();
            parser.parse_move().map(|m| Action::Edit { e: Edit::Delete(m), count: None })
          }
        }
      }
      (Mode::Normal, Key::Char('c')) => {
        match parser.remaining.get(1).ok_or(ActionError::Incomplete)? {
          KeyStroke { key: Key::Char('c'), .. } => e!(CutLine),
          _ => {
            parser.skip();
            parser.parse_move().map(|m| Action::Edit { e: Edit::Cut(m), count: None })
          }
        }
      }
      (Mode::Normal, Key::Char('D')) => e!(DeleteRestOfLine),
      (Mode::Normal, Key::Char('p')) => e!(Paste { after: true }),
      (Mode::Normal, Key::Char('P')) => e!(Paste { after: false }),
      (Mode::Normal, Key::Char('u')) => e!(Undo),
      (Mode::Normal, Key::Char('r')) if key.control => e!(Redo),
      (Mode::Normal, Key::Char('~')) => e!(SwitchCase),

      // === visual ===
      (Mode::Visual(_), Key::Char('y' | 'Y')) => Ok(Action::Yank),
      (Mode::Visual(_), Key::Char('x' | 'd')) => e!(Delete(Move::Single(Direction::Right))),

      // === modes ===
      (Mode::Normal, Key::Char('i')) => Ok(Action::SetMode { mode: Mode::Insert, delta: 0 }),
      (Mode::Normal, Key::Char('a')) => Ok(Action::SetMode { mode: Mode::Insert, delta: 1 }),
      (Mode::Normal, Key::Char('o')) => Ok(Action::Append { after: true }),
      (Mode::Normal, Key::Char('O')) => Ok(Action::Append { after: false }),
      (Mode::Visual(VisualMode::Block), Key::Char('v')) if key.control => {
        Ok(Action::SetMode { mode: Mode::Normal, delta: 0 })
      }
      (Mode::Normal | Mode::Visual(_), Key::Char('v')) if key.control => {
        Ok(Action::SetMode { mode: Mode::Visual(VisualMode::Block), delta: 0 })
      }
      (Mode::Visual(VisualMode::Character), Key::Char('v')) => {
        Ok(Action::SetMode { mode: Mode::Normal, delta: 0 })
      }
      (Mode::Normal | Mode::Visual(_), Key::Char('v')) => {
        Ok(Action::SetMode { mode: Mode::Visual(VisualMode::Character), delta: 0 })
      }
      (Mode::Visual(VisualMode::Line), Key::Char('V')) => {
        Ok(Action::SetMode { mode: Mode::Normal, delta: 0 })
      }
      (Mode::Normal | Mode::Visual(_), Key::Char('V')) => {
        Ok(Action::SetMode { mode: Mode::Visual(VisualMode::Line), delta: 0 })
      }
      (Mode::Normal, Key::Char('R')) => Ok(Action::SetMode { mode: Mode::Replace, delta: 0 }),
      (Mode::Normal, Key::Char(':')) => Ok(Action::SetMode { mode: Mode::Command, delta: 0 }),
      (Mode::Normal, Key::Char('/')) => Ok(Action::OpenSearch),

      (Mode::Normal | Mode::Visual(_), _) => {
        parser.parse_move().map(|m| Action::Move { count: NonZero::new(count), m })
      }

      _ => Err(ActionError::Unrecognized),
    };
  }
}

impl KeyParser<'_> {
  fn skip(&mut self) {
    if self.remaining.is_empty() {
      panic!("cannot skip with no keys remaining");
    }
    self.remaining = &self.remaining[1..];
  }

  fn parse_char(&mut self) -> Result<char, ActionError> {
    let Key::Char(c) = self.remaining.get(0).ok_or(ActionError::Incomplete)?.key else {
      return Err(ActionError::Unrecognized);
    };

    self.remaining = &self.remaining[1..];
    Ok(c)
  }

  fn parse_move(&mut self) -> Result<be_input::Move, ActionError> {
    let (m, n) = self.keybinds.movement.get(&self.remaining)?;
    self.remaining = &self.remaining[n..];
    self.translate_move(m)
  }

  fn translate_action(&mut self, a: &grammar::Action) -> Result<be_input::Action, ActionError> {
    use crate::grammar::Action as G;
    use be_input::Action::*;

    Ok(match *a {
      G::SetMode { mode, delta } => SetMode { mode, delta },
      G::OpenSearch => OpenSearch,
      G::Append { after } => Append { after },
      G::Edit { ref edit } => Edit { count: None, e: self.translate_edit(edit)? },
      G::Navigate { nav } => Navigate { nav },
      G::Autocomplete => Autocomplete,
      G::MoveCompletion { next } => MoveCompletion { next },
      G::Yank => Yank,
      G::ClipboardCopy => ClipboardCopy,
      G::ClipboardPaste => ClipboardPaste,
    })
  }

  fn translate_edit(&mut self, e: &grammar::Edit) -> Result<be_input::Edit, ActionError> {
    use crate::grammar::Edit as G;
    use be_input::Edit::*;

    Ok(match *e {
      G::Replace => Replace(self.parse_char()?),
      G::Delete => Delete(self.parse_move()?),
      G::Cut => Cut(self.parse_move()?),
      G::DeleteLine => DeleteLine,
      G::CutLine => CutLine,
      G::DeleteRestOfLine => DeleteRestOfLine,
      G::Paste { after } => Paste { after },
      G::Backspace => Backspace,
      G::Undo => Undo,
      G::Redo => Redo,
      G::SwitchCase => SwitchCase,
    })
  }

  fn translate_move(&mut self, m: &grammar::Move) -> Result<be_input::Move, ActionError> {
    use crate::grammar::Move as G;
    use be_input::Move::*;

    Ok(match *m {
      G::Single(d) => Single(d),
      G::NextWord => NextWord,
      G::EndWord => EndWord,
      G::PrevWord => PrevWord,
      G::LineStart => LineStart,
      G::LineStartOfText => LineStartOfText,
      G::LineEnd => LineEnd,
      G::MatchingBracket => MatchingBracket,
      G::FileStart => FileStart,
      G::FileEnd => FileEnd,
      G::Result(d) => Result(d),
      G::Change(d) => Change(d),
      G::Diagnostic(d) => Diagnostic(d),
      G::GotoDefinition => GotoDefinition,
      G::GotoReference => GotoReference,
      G::Char(d) => Char(self.parse_char()?, d),
      G::BackDefinition => BackDefinition,
    })
  }
}

impl<T> KeyMap<T> {
  fn get(&self, seq: &[KeyStroke]) -> Result<(&T, usize), ActionError> {
    if seq.is_empty() {
      return Err(ActionError::Incomplete);
    }

    let mut found_partial = false;

    for (k, v) in self.keys.iter() {
      if k.0.starts_with(seq) {
        found_partial = true;
      }

      if seq.starts_with(&k.0) {
        return Ok((v, k.0.len()));
      }
    }

    Err(if found_partial { ActionError::Incomplete } else { ActionError::Unrecognized })
  }
}
