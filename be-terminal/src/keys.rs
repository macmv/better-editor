//! Encoding of keyboard input into the bytes a program expects. These only
//! depend on the input modes the program has enabled, so they work for
//! anything that has an [`Emulator`](crate::Emulator), not just `Terminal`.

use crate::{
  KITTY_ALL_AS_ESCAPES, KITTY_ALTERNATE_KEYS, KITTY_ASSOCIATED_TEXT, KITTY_DISAMBIGUATE, control,
};

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

/// Modifier bits, in the order the kitty keyboard protocol numbers them.
pub mod mods {
  pub const SHIFT: u8 = 1;
  pub const ALT: u8 = 2;
  pub const CTRL: u8 = 4;
}

/// A key that the kitty keyboard protocol can encode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
  /// A key that types text. This is the character the key produced, so
  /// Shift+a is `Char('A')`.
  Char(char),
  Enter,
  Tab,
  Backspace,
  Escape,
}

/// Encodes a key press for a program with the given input modes.
///
/// With no kitty flags this produces the legacy bytes. With the disambiguate
/// flag, keys that are ambiguous in the legacy encoding (Escape, and anything
/// with Ctrl or Alt held) use `CSI u`. With the report-all-keys flag, every
/// key does.
pub fn encode(key: Key, mods: u8, modes: InputModes) -> Vec<u8> {
  let flags = modes.kitty_flags;
  let all = flags & KITTY_ALL_AS_ESCAPES != 0;
  let disambiguate = flags & KITTY_DISAMBIGUATE != 0;

  // Control characters are sent as-is, they have no `CSI u` form.
  if let Key::Char(c) = key
    && c.is_control()
  {
    return input_legacy(c);
  }

  let escape = all
    || (disambiguate
      && (key == Key::Escape
        || mods & (mods::CTRL | mods::ALT) != 0
        || (mods & mods::SHIFT != 0 && !matches!(key, Key::Char(_)))));

  if !escape {
    return legacy(key, mods);
  }

  let (code, mut mods, alternate) = match key {
    Key::Char(c) if c.is_uppercase() => {
      // The protocol reports the unshifted key, with shift as a modifier.
      let lower = c.to_lowercase().next().unwrap_or(c);
      (lower as u32, mods | mods::SHIFT, (flags & KITTY_ALTERNATE_KEYS != 0).then_some(c as u32))
    }
    Key::Char(c) => (c as u32, mods, None),
    Key::Enter => (13, mods, None),
    Key::Tab => (9, mods, None),
    Key::Backspace => (127, mods, None),
    Key::Escape => (27, mods, None),
  };
  mods &= mods::SHIFT | mods::ALT | mods::CTRL;

  // Text is only reported for keys that produce it.
  let text = match key {
    Key::Char(c)
      if all && flags & KITTY_ASSOCIATED_TEXT != 0 && mods & (mods::CTRL | mods::ALT) == 0 =>
    {
      Some(c as u32)
    }
    _ => None,
  };

  let mut out = format!("\x1b[{code}");
  if let Some(alt) = alternate {
    out.push_str(&format!(":{alt}"));
  }
  if mods != 0 || text.is_some() {
    out.push_str(&format!(";{}", mods + 1));
  }
  if let Some(text) = text {
    out.push_str(&format!(";{text}"));
  }
  out.push('u');
  out.into_bytes()
}

fn input_legacy(c: char) -> Vec<u8> { c.encode_utf8(&mut [0u8; 4]).as_bytes().to_vec() }

fn legacy(key: Key, mods: u8) -> Vec<u8> {
  let mut out = match key {
    Key::Char(c) if mods & mods::CTRL != 0 && c.is_ascii_alphabetic() => {
      vec![c.to_ascii_lowercase() as u8 - b'a' + 1]
    }
    Key::Char(c) => input_legacy(c),
    Key::Enter => vec![b'\r'],
    Key::Tab if mods & mods::SHIFT != 0 => b"\x1b[Z".to_vec(),
    Key::Tab => vec![control::C0::HT],
    Key::Backspace => vec![control::C0::DEL],
    Key::Escape => vec![control::C0::ESC],
  };
  if mods & mods::ALT != 0 {
    out.insert(0, control::C0::ESC);
  }
  out
}

/// Typed text. `\r`, `\t`, and so on are treated as the Enter, Tab, ...
/// keys.
pub fn input_with(c: char, modes: InputModes) -> Vec<u8> {
  let key = match c {
    '\r' => Key::Enter,
    '\t' => Key::Tab,
    '\x7f' => Key::Backspace,
    '\x1b' => Key::Escape,
    c => Key::Char(c),
  };
  encode(key, 0, modes)
}

/// A Ctrl+letter byte (1 for Ctrl+A, ...). Other bytes are sent as-is.
pub fn control_with(b: u8, modes: InputModes) -> Vec<u8> {
  match b {
    1..=26 => encode(Key::Char((b - 1 + b'a') as char), mods::CTRL, modes),
    _ => vec![b],
  }
}

pub fn delete() -> Vec<u8> { vec![control::C0::DEL] }

/// Ctrl+Enter. Plain terminals can't tell this apart from Enter, so it only
/// gets a distinct encoding if the program asked for it with the kitty
/// keyboard protocol.
pub fn ctrl_enter(modes: InputModes) -> Vec<u8> { encode(Key::Enter, mods::CTRL, modes) }

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
    assert_eq!(input_with('\u{e9}', InputModes::default()), "\u{e9}".as_bytes());
    assert_eq!(delete(), [0x7f]);
    assert_eq!(encode(Key::Backspace, 0, InputModes::default()), [0x7f]);
  }

  #[test]
  fn ctrl_enter_needs_kitty() {
    assert_eq!(ctrl_enter(InputModes::default()), b"\r");
    let kitty = InputModes { kitty_flags: KITTY_DISAMBIGUATE, ..Default::default() };
    assert_eq!(ctrl_enter(kitty), b"\x1b[13;5u");
  }

  fn kitty(flags: u8) -> InputModes { InputModes { kitty_flags: flags, ..Default::default() } }

  #[test]
  fn legacy_encoding_without_flags() {
    let m = InputModes::default();
    assert_eq!(input_with('a', m), b"a");
    assert_eq!(input_with('\r', m), b"\r");
    assert_eq!(input_with('\x1b', m), b"\x1b");
    assert_eq!(control_with(3, m), [3]);
    assert_eq!(control_with(28, m), [28]);
    assert_eq!(encode(Key::Tab, mods::SHIFT, m), b"\x1b[Z");
    assert_eq!(encode(Key::Char('a'), mods::ALT, m), b"\x1ba");
  }

  #[test]
  fn disambiguate() {
    let m = kitty(KITTY_DISAMBIGUATE);
    assert_eq!(input_with('\x1b', m), b"\x1b[27u");
    assert_eq!(control_with(3, m), b"\x1b[99;5u");
    assert_eq!(encode(Key::Char('a'), mods::ALT, m), b"\x1b[97;3u");
    assert_eq!(encode(Key::Tab, mods::SHIFT, m), b"\x1b[9;2u");
    // Plain and shifted text is unchanged, as are Enter, Tab, and Backspace.
    assert_eq!(input_with('a', m), b"a");
    assert_eq!(input_with('A', m), b"A");
    assert_eq!(input_with('\r', m), b"\r");
    assert_eq!(input_with('\t', m), b"\t");
    assert_eq!(input_with('\x7f', m), [0x7f]);
  }

  #[test]
  fn report_all_keys() {
    let m = kitty(KITTY_DISAMBIGUATE | KITTY_ALL_AS_ESCAPES);
    assert_eq!(input_with('a', m), b"\x1b[97u");
    assert_eq!(input_with('A', m), b"\x1b[97;2u");
    assert_eq!(input_with('\r', m), b"\x1b[13u");
    assert_eq!(input_with('\t', m), b"\x1b[9u");
    assert_eq!(input_with('\x7f', m), b"\x1b[127u");

    let text = kitty(KITTY_DISAMBIGUATE | KITTY_ALL_AS_ESCAPES | KITTY_ASSOCIATED_TEXT);
    assert_eq!(input_with('a', text), b"\x1b[97;1;97u");
    assert_eq!(input_with('A', text), b"\x1b[97;2;65u");
    assert_eq!(control_with(1, text), b"\x1b[97;5u");

    let alt = kitty(KITTY_ALL_AS_ESCAPES | KITTY_ALTERNATE_KEYS);
    assert_eq!(input_with('A', alt), b"\x1b[97:65;2u");
  }

  #[test]
  fn paste_modes() {
    assert_eq!(paste("a\r\nb\nc", InputModes::default()), b"a\rb\rc");
    let bracketed = InputModes { bracketed_paste: true, ..Default::default() };
    assert_eq!(paste("a\x1b\x03b\n", bracketed), b"\x1b[200~ab\n\x1b[201~");
  }
}
