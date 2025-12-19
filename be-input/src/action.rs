use std::num::NonZero;

use crate::{Mode, key::Key};

pub enum Action {
  SetMode(Mode),
  Move { count: Option<NonZero<u32>>, m: Move },
  Edit { count: Option<NonZero<u32>>, e: Edit },
}

pub enum Move {
  Left,
  Right,
  Up,
  Down,

  NextWord,
  EndWord,
  PrevWord,
  Backward(char),
  Forward(char),

  LineStart,
  LineStartOfText,
  LineEnd,
  MatchingBracket,

  FileStart,
  FileEnd,
}

pub enum Edit {
  Insert(char),
  Replace(char),
  Delete,
  Backspace,
}

pub enum ActionError {
  Unrecognized,
  Incomplete,
}

impl Action {
  pub fn from_input(mode: Mode, input: &[Key]) -> Result<Action, ActionError> {
    let mut count = 0;

    macro_rules! e {
      ($($e:tt)*) => {
        Ok(Action::Edit { count: NonZero::new(count), e: Edit::$($e)* })
      };
    }

    let mut iter = input.iter().copied();

    while let Some(key) = iter.next() {
      return match (mode, key) {
        (Mode::Insert | Mode::Command, Key::Char(c)) => e!(Insert(c)),
        (Mode::Insert | Mode::Command, Key::Backspace) => e!(Backspace),
        (Mode::Insert | Mode::Command, Key::Escape) => Ok(Action::SetMode(Mode::Normal)),

        (Mode::Normal, Key::Char(c @ '1'..='9')) => {
          count += u32::from(c) - u32::from('0');

          continue;
        }

        // === edits ===
        (Mode::Normal, Key::Char('r')) => match iter.next().ok_or(ActionError::Incomplete)? {
          Key::Char(c) => e!(Replace(c)),
          _ => Err(ActionError::Unrecognized),
        },
        (Mode::Normal, Key::Char('x')) => e!(Delete),

        // === modes ===
        (Mode::Normal, Key::Char('i')) => Ok(Action::SetMode(Mode::Insert)),
        (Mode::Normal, Key::Char('a')) => Ok(Action::SetMode(Mode::Insert)),
        (Mode::Normal, Key::Char('v')) => Ok(Action::SetMode(Mode::Visual)),
        (Mode::Normal, Key::Char('R')) => Ok(Action::SetMode(Mode::Replace)),
        (Mode::Normal, Key::Char(':')) => Ok(Action::SetMode(Mode::Command)),

        (Mode::Normal | Mode::Visual, _) => {
          parse_move(key, iter).map(|m| Action::Move { count: NonZero::new(count), m })
        }

        _ => Err(ActionError::Unrecognized),
      };
    }

    Err(ActionError::Incomplete)
  }
}

fn parse_move(key: Key, mut iter: impl Iterator<Item = Key>) -> Result<Move, ActionError> {
  use Move::*;

  Ok(match key {
    Key::Char('h') => Left,
    Key::Char('j') => Down,
    Key::Char('k') => Up,
    Key::Char('l') => Right,
    Key::Char('w') => NextWord,
    Key::Char('e') => EndWord,
    Key::Char('b') => PrevWord,
    Key::Char('0') => LineStart,
    Key::Char('^') => LineStartOfText,
    Key::Char('$') => LineEnd,
    Key::Char('%') => MatchingBracket,
    Key::Char('g') => match iter.next().ok_or(ActionError::Incomplete)? {
      Key::Char('g') => FileStart,
      _ => return Err(ActionError::Unrecognized),
    },
    Key::Char('G') => FileEnd,
    Key::Char('f') => match iter.next().ok_or(ActionError::Incomplete)? {
      Key::Char(c) => Forward(c),
      _ => return Err(ActionError::Unrecognized),
    },
    Key::Char('F') => match iter.next().ok_or(ActionError::Incomplete)? {
      Key::Char(c) => Backward(c),
      _ => return Err(ActionError::Unrecognized),
    },
    Key::Backspace => Left,

    _ => return Err(ActionError::Unrecognized),
  })
}
