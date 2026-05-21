use std::num::NonZero;

use crate::Mode;

#[derive(Debug)]
pub enum Action {
  SetMode { mode: Mode, delta: i32 },
  OpenSearch,
  Append { after: bool },
  Move { count: Option<NonZero<u32>>, m: Move },
  Edit { count: Option<NonZero<u32>>, e: Edit },
  Tab { shift: bool },
  Control { char: char },
  Navigate { nav: Navigation },
  Autocomplete,
  MoveCompletion { next: bool },
  Yank,
  ClipboardCopy,
  ClipboardPaste,
}

#[derive(Debug)]
pub enum Navigation {
  OpenWorkspace,
  OpenSearch,
  Direction(Direction),
  Split(Axis),
  Tab(u8),
}

#[derive(Debug, Clone, Copy)]
pub enum Axis {
  Horizontal,
  Vertical,
}

#[derive(Debug)]
pub enum Move {
  Single(Direction),

  NextWord,
  EndWord,
  PrevWord,
  Char(char, ChangeDirection),

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

#[derive(Debug, Copy, Clone)]
pub enum ChangeDirection {
  Next,
  Prev,
}

#[derive(Debug)]
pub enum Edit {
  Insert(char),
  Replace(char),
  Delete(Move),
  Cut(Move),
  DeleteLine,
  CutLine,
  DeleteRestOfLine,
  Paste { after: bool },
  Backspace,
  Undo,
  Redo,
  SwitchCase,
}

#[derive(Debug)]
pub enum ActionError {
  Unrecognized,
  Incomplete,
}

#[derive(Debug, Copy, Clone)]
pub enum Direction {
  Up,
  Down,
  Left,
  Right,
}

#[derive(Copy, Clone)]
pub enum VerticalDirection {
  Up,
  Down,
}
