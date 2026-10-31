use std::num::NonZero;

use be_input::{ActionError, ChangeDirection, Direction, Key, KeyStroke, Mode, Navigation};

use crate::{
  grammar,
  keybind::{KeyMap, Keybinds},
};

#[derive(Debug, Clone)]
pub enum Action {
  SetMode { mode: Mode, delta: i32 },
  OpenSearch,
  Append { after: bool },
  Edit { edit: Edit },
  Autocomplete,
  MoveCompletion { next: bool },
  Navigate { nav: Navigation },
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

#[derive(Debug, Clone)]
pub enum Edit {
  Replace,
  Delete,
  DeleteSingle,
  Cut,
  DeleteLine,
  CutLine,
  DeleteRestOfLine,
  Paste { after: bool },
  Backspace,
  Insert(char),
  Undo,
  Redo,
  SwitchCase,
}

struct KeyParser<'a> {
  keybinds: &'a Keybinds,
  mode:     Mode,

  count:     u32,
  remaining: &'a [KeyStroke],
}

impl Keybinds {
  pub fn parse(&self, mode: Mode, input: &[KeyStroke]) -> Result<be_input::Action, ActionError> {
    use be_input::*;

    let mut parser = KeyParser { keybinds: self, mode, count: 0, remaining: input };
    parser.parse_count();

    if parser.remaining.is_empty() {
      return Err(ActionError::Incomplete);
    }

    match parser.parse_action(&self.global) {
      Ok(action) => return Ok(action),
      Err(ActionError::Incomplete) => return Err(ActionError::Incomplete),
      Err(ActionError::Unrecognized) => {}
    }

    if let Some(nav_key) = self.settings.navigate.0.get(0)
      && parser.remaining[0] == *nav_key
    {
      parser.skip();
      return parser.parse_action(&self.navigation);
    }

    macro_rules! parse_chain {
      ($e:expr$(, $rest:expr)* $(,)?) => {
        match $e {
          Ok(action) => action,
          Err(ActionError::Incomplete) => return Err(ActionError::Incomplete),
          Err(ActionError::Unrecognized) => parse_chain!($($rest),*),
        }
      };

      () => {
        return Err(ActionError::Unrecognized)
      };
    }

    let action = match mode {
      Mode::Normal => parse_chain!(
        parser.parse_action(&self.normal),
        parser.parse_move().map(|m| Action::Move { count: NonZero::new(parser.count), m })
      ),
      Mode::Visual(VisualMode::Line) => parse_chain!(
        parser.parse_action(&self.visual.line),
        parser.parse_action(&self.visual.all),
        parser.parse_move().map(|m| Action::Move { count: NonZero::new(parser.count), m })
      ),
      Mode::Visual(VisualMode::Block) => parse_chain!(
        parser.parse_action(&self.visual.block),
        parser.parse_action(&self.visual.all),
        parser.parse_move().map(|m| Action::Move { count: NonZero::new(parser.count), m })
      ),
      Mode::Visual(VisualMode::Character) => parse_chain!(
        parser.parse_action(&self.visual.character),
        parser.parse_action(&self.visual.all),
        parser.parse_move().map(|m| Action::Move { count: NonZero::new(parser.count), m })
      ),
      Mode::Insert | Mode::Command => {
        let key = parser.remaining[0];
        match key.key {
          #[cfg(not(target_os = "macos"))]
          Key::Char('C') if key.control => Action::ClipboardCopy,
          #[cfg(not(target_os = "macos"))]
          Key::Char('V') if key.control => Action::ClipboardPaste,
          #[cfg(target_os = "macos")]
          Key::Char('c') if key.sup => Action::ClipboardCopy,
          #[cfg(target_os = "macos")]
          Key::Char('v') if key.sup => Action::ClipboardPaste,

          Key::Char(' ') if key.control => Action::Autocomplete,
          Key::Char('p') if key.control => Action::MoveCompletion { next: false },
          Key::Char('n') if key.control => Action::MoveCompletion { next: true },
          Key::Char(c) if key.control => Action::Control { char: c },
          Key::Tab => Action::Tab { shift: key.shift },

          Key::Char(c) => Action::Edit { count: None, e: Edit::Insert(c) },
          Key::Backspace => Action::Edit { count: None, e: Edit::Backspace },
          Key::Escape => Action::SetMode { mode: Mode::Normal, delta: -1 },

          Key::ArrowUp => Action::Move { count: None, m: Move::Single(Direction::Up) },
          Key::ArrowDown => Action::Move { count: None, m: Move::Single(Direction::Down) },
          Key::ArrowLeft => Action::Move { count: None, m: Move::Single(Direction::Left) },
          Key::ArrowRight => Action::Move { count: None, m: Move::Single(Direction::Right) },

          _ => return Err(ActionError::Unrecognized),
        }
      }
      _ => return Err(ActionError::Unrecognized),
    };

    Ok(action)
  }
}

impl KeyParser<'_> {
  fn skip(&mut self) {
    if self.remaining.is_empty() {
      panic!("cannot skip with no keys remaining");
    }
    self.remaining = &self.remaining[1..];
  }

  fn parse_count(&mut self) {
    if matches!(self.mode, Mode::Normal | Mode::Visual(_)) {
      while let Some(stroke) = self.remaining.get(0)
        && !stroke.control
        && !stroke.alt
        && !stroke.sup
        && let Key::Char(c @ '0'..='9') = stroke.key
      {
        // Counts cannot start with 0 (as 0 is start-of-line).
        if c == '0' && self.count == 0 {
          break;
        }

        self.count = self.count.saturating_mul(10).saturating_add(u32::from(c) - u32::from('0'));
        self.skip();
      }
    }
  }

  fn parse_char(&mut self) -> Result<char, ActionError> {
    let Key::Char(c) = self.remaining.get(0).ok_or(ActionError::Incomplete)?.key else {
      return Err(ActionError::Unrecognized);
    };

    self.remaining = &self.remaining[1..];
    Ok(c)
  }

  fn parse_action(
    &mut self,
    keybinds: &KeyMap<grammar::Action>,
  ) -> Result<be_input::Action, ActionError> {
    let (a, n) = keybinds.get(&self.remaining)?;
    self.remaining = &self.remaining[n..];
    self.translate_action(a)
  }

  fn parse_move(&mut self) -> Result<be_input::Move, ActionError> {
    let (m, n) = self.keybinds.movement.get(&self.remaining)?;
    self.remaining = &self.remaining[n..];
    self.translate_move(m)
  }

  fn parse_move_for_edit(&mut self) -> Result<be_input::Move, ActionError> {
    match self.mode {
      Mode::Normal => self.parse_move(),
      _ => Ok(be_input::Move::Single(Direction::Right)),
    }
  }

  fn translate_action(&mut self, a: &grammar::Action) -> Result<be_input::Action, ActionError> {
    use crate::grammar::Action as G;
    use be_input::Action::*;

    Ok(match *a {
      G::SetMode { mode, delta } => SetMode { mode, delta },
      G::OpenSearch => OpenSearch,
      G::Append { after } => Append { after },
      G::Edit { ref edit } => Edit { count: None, e: self.translate_edit(edit)? },
      G::Autocomplete => Autocomplete,
      G::MoveCompletion { next } => MoveCompletion { next },
      G::Navigate { nav } => Navigate { nav },
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
      G::Delete => Delete(self.parse_move_for_edit()?),
      G::Cut => Cut(self.parse_move_for_edit()?),
      G::DeleteSingle => Delete(be_input::Move::Single(Direction::Right)),
      G::DeleteLine => DeleteLine,
      G::CutLine => CutLine,
      G::DeleteRestOfLine => Delete(be_input::Move::LineEnd),
      G::Paste { after } => Paste { after },
      G::Backspace => Backspace,
      G::Insert(c) => Insert(c),
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
    let mut best_match = None;

    for (k, v) in self.keys.iter() {
      if k.0.starts_with(seq) {
        found_partial = true;
      }

      if seq.starts_with(&k.0) && best_match.is_none_or(|(_, l)| l < k.0.len()) {
        best_match = Some((v, k.0.len()));
      }
    }

    if let Some(best) = best_match {
      Ok(best)
    } else if found_partial {
      Err(ActionError::Incomplete)
    } else {
      Err(ActionError::Unrecognized)
    }
  }
}
