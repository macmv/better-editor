#![allow(dead_code)]

use be_input::{ChangeDirection, Direction, Mode, Navigation};

pub enum Action {
  SetMode { mode: Mode, delta: i32 },
  OpenSearch,
  Append { after: bool },
  Move { movement: Move },
  Edit { edit: Edit },
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
  Insert,
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
