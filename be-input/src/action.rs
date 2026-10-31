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

#[derive(Debug, Clone, Copy)]
pub enum Navigation {
  /// Toggle between the home terminal and the active workspace.
  OpenWorkspace,
  /// Toggle the screen listing every open workspace.
  Workspaces,
  OpenSearch,
  OpenSymbolSearch,
  Direction(Direction),
  /// Focus the next or previous pane in the current tab, wrapping around.
  Cycle(ChangeDirection),
  /// Focus the previously focused pane in the current tab.
  LastPane,
  ClosePane,
  /// Toggle the active pane filling the whole tab.
  Zoom,
  Split(Axis),
  Tab(u8),
  /// Open a new tab containing a terminal, and switch to it.
  NewTerminal,
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
