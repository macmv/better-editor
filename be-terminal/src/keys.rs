//! Encoding of keyboard input into the bytes a program expects. These only
//! depend on the input modes the program has enabled, so they work for
//! anything that has an [`Emulator`](crate::Emulator), not just `Terminal`.

use crate::{KITTY_DISAMBIGUATE, control};

/// The modes a program can set that change how keys are encoded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputModes {
  /// Arrow keys send `ESC O A` instead of `ESC [ A` (DECCKM).
  pub application_cursor: bool,
  /// The active kitty keyboard protocol flags.
  pub kitty_flags:        u8,
  pub bracketed_paste:    bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
  Up,
  Down,
  Left,
  Right,
}

pub fn input(c: char) -> Vec<u8> { c.encode_utf8(&mut [0u8; 4]).as_bytes().to_vec() }

pub fn backspace() -> Vec<u8> { vec![control::C0::DEL] }

pub fn delete() -> Vec<u8> { vec![control::C0::DEL] }

pub fn control(b: u8) -> Vec<u8> { vec![b] }

/// Ctrl+Enter. Plain terminals can't tell this apart from Enter, so it only
/// gets a distinct encoding if the program asked for it with the kitty
/// keyboard protocol.
pub fn ctrl_enter(modes: InputModes) -> Vec<u8> {
  if modes.kitty_flags & KITTY_DISAMBIGUATE != 0 { b"\x1b[13;5u".to_vec() } else { b"\r".to_vec() }
}

pub fn escape() -> Vec<u8> { vec![control::C0::ESC] }

pub fn tab() -> Vec<u8> { vec![control::C0::HT] }

pub fn shift_tab() -> Vec<u8> { b"\x1b[Z".to_vec() }

pub fn arrow(arrow: Arrow, modes: InputModes) -> Vec<u8> {
  let c = match arrow {
    Arrow::Up => b'A',
    Arrow::Down => b'B',
    Arrow::Right => b'C',
    Arrow::Left => b'D',
  };
  vec![0x1b, if modes.application_cursor { b'O' } else { b'[' }, c]
}

/// Pasted text. With bracketed paste on, the program can tell it apart from
/// typing.
pub fn paste(s: &str, modes: InputModes) -> Vec<u8> {
  // Source: alacritty
  if modes.bracketed_paste {
    let mut out = b"\x1b[200~".to_vec();

    // We remove `\x1b` to ensure it's impossible for the pasted text to write
    // the bracketed paste end escape `\x1b[201~` and `\x03` since some
    // shells incorrectly terminate bracketed paste when they receive
    // it.
    out.extend(s.replace(['\x1b', '\x03'], "").as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
  } else {
    // In non-bracketed (ie: normal) mode, terminal applications cannot
    // distinguish pasted data from keystrokes.
    //
    // In theory, we should construct the keystrokes needed to produce the
    // data we are pasting... since that's neither practical nor
    // sensible (and probably an impossible task to solve in a general
    // way), we'll just replace line breaks (windows and unix style)
    // with a single carriage return (\r, which is what the Enter key
    // produces).
    s.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn arrows_follow_cursor_key_mode() {
    let normal = InputModes::default();
    let app = InputModes { application_cursor: true, ..normal };
    assert_eq!(arrow(Arrow::Up, normal), b"\x1b[A");
    assert_eq!(arrow(Arrow::Down, app), b"\x1bOB");
    assert_eq!(arrow(Arrow::Right, normal), b"\x1b[C");
    assert_eq!(arrow(Arrow::Left, app), b"\x1bOD");
  }

  #[test]
  fn simple_keys() {
    assert_eq!(input('a'), b"a");
    assert_eq!(input('\u{e9}'), "\u{e9}".as_bytes());
    assert_eq!(backspace(), [0x7f]);
    assert_eq!(delete(), [0x7f]);
    assert_eq!(control(3), [3]);
    assert_eq!(escape(), [0x1b]);
    assert_eq!(tab(), [b'\t']);
    assert_eq!(shift_tab(), b"\x1b[Z");
  }

  #[test]
  fn ctrl_enter_needs_kitty() {
    assert_eq!(ctrl_enter(InputModes::default()), b"\r");
    let kitty = InputModes { kitty_flags: KITTY_DISAMBIGUATE, ..Default::default() };
    assert_eq!(ctrl_enter(kitty), b"\x1b[13;5u");
  }

  #[test]
  fn paste_modes() {
    assert_eq!(paste("a\r\nb\nc", InputModes::default()), b"a\rb\rc");
    let bracketed = InputModes { bracketed_paste: true, ..Default::default() };
    assert_eq!(paste("a\x1b\x03b\n", bracketed), b"\x1b[200~ab\n\x1b[201~");
  }
}
