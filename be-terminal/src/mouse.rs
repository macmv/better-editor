//! Encoding of mouse and focus events into the bytes a program expects. Like
//! [`keys`](crate::keys), these only depend on the modes the program has
//! enabled, so they work for anything that has an
//! [`Emulator`](crate::Emulator).

use crate::MouseMode;

/// The modes a program can set that change which mouse events it gets and how
/// they are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseModes {
  /// Button presses and releases (mode 1000).
  pub reporting: bool,
  /// Which motion is reported (modes 1002 and 1003).
  pub motion:    MouseMode,
  /// Coordinates are UTF-8 codepoints (mode 1005).
  pub utf8:      bool,
  /// Focus changes are reported (mode 1004).
  pub focus:     bool,
}

impl Default for MouseModes {
  fn default() -> Self {
    MouseModes { reporting: false, motion: MouseMode::None, utf8: false, focus: false }
  }
}

/// A press or release of `button` (0=left, 1=middle, 2=right) at a cell.
/// Only reported when mode 1000 is active.
pub fn button(
  modes: MouseModes,
  col: usize,
  row: usize,
  button: u8,
  pressed: bool,
) -> Option<Vec<u8>> {
  if !modes.reporting {
    return None;
  }
  report(modes, col, row, if pressed { button } else { 3 })
}

/// A scroll-wheel event: `dy > 0` is up. Only reported when mode 1000 is
/// active.
pub fn scroll(modes: MouseModes, col: usize, row: usize, dy: i32) -> Option<Vec<u8>> {
  if !modes.reporting {
    return None;
  }
  // Scroll buttons: 64=wheel-up, 65=wheel-down.
  report(modes, col, row, if dy > 0 { 64 } else { 65 })
}

/// Mouse motion, with the button held (0=left, 1=middle, 2=right) if any.
/// Reported for mode 1003 (all motion), or mode 1002 when a button is held.
pub fn motion(modes: MouseModes, col: usize, row: usize, held: Option<u8>) -> Option<Vec<u8>> {
  let wanted = match modes.motion {
    MouseMode::Motion => true,
    MouseMode::Drag => held.is_some(),
    MouseMode::None | MouseMode::Press => false,
  };
  if !wanted {
    return None;
  }
  // The motion bit (32) is added to the button code. With no button held the
  // code is 3 (release), giving 35.
  report(modes, col, row, held.map(|b| b | 32).unwrap_or(32 | 3))
}

/// Focus gained or lost. Only reported when mode 1004 is active.
pub fn focus(modes: MouseModes, focused: bool) -> Option<Vec<u8>> {
  modes.focus.then(|| if focused { b"\x1b[I".to_vec() } else { b"\x1b[O".to_vec() })
}

fn report(modes: MouseModes, col: usize, row: usize, code: u8) -> Option<Vec<u8>> {
  let mut buf = vec![0x1b, b'[', b'M'];
  if modes.utf8 {
    // Mode 1005: each of the three values is a UTF-8 codepoint.
    push_utf8_coord(&mut buf, 32 + u32::from(code));
    push_utf8_coord(&mut buf, 32 + col as u32 + 1);
    push_utf8_coord(&mut buf, 32 + row as u32 + 1);
  } else {
    // X10 encoding: coordinates are limited to < 224 (byte value 255-32+1).
    if col >= 224 || row >= 224 {
      return None;
    }
    buf.extend_from_slice(&[32 + code, (32 + col + 1) as u8, (32 + row + 1) as u8]);
  }
  Some(buf)
}

fn push_utf8_coord(buf: &mut Vec<u8>, codepoint: u32) {
  if let Some(c) = char::from_u32(codepoint) {
    let mut tmp = [0u8; 4];
    buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn modes(motion: MouseMode) -> MouseModes {
    MouseModes { reporting: true, motion, ..Default::default() }
  }

  #[test]
  fn buttons_and_wheel() {
    let m = modes(MouseMode::Press);
    assert_eq!(button(m, 0, 0, 0, true), Some(vec![0x1b, b'[', b'M', 32, 33, 33]));
    assert_eq!(button(m, 4, 9, 2, true), Some(vec![0x1b, b'[', b'M', 34, 37, 42]));
    // Releases are button 3.
    assert_eq!(button(m, 0, 0, 0, false), Some(vec![0x1b, b'[', b'M', 35, 33, 33]));
    assert_eq!(scroll(m, 0, 0, 1), Some(vec![0x1b, b'[', b'M', 96, 33, 33]));
    assert_eq!(scroll(m, 0, 0, -1), Some(vec![0x1b, b'[', b'M', 97, 33, 33]));
    assert_eq!(button(MouseModes::default(), 0, 0, 0, true), None);
    assert_eq!(scroll(MouseModes::default(), 0, 0, 1), None);
  }

  #[test]
  fn motion_depends_on_the_mode() {
    assert_eq!(motion(modes(MouseMode::Press), 1, 1, Some(0)), None);
    assert_eq!(motion(modes(MouseMode::Drag), 1, 1, None), None);
    assert_eq!(
      motion(modes(MouseMode::Drag), 1, 1, Some(0)),
      Some(vec![0x1b, b'[', b'M', 64, 34, 34])
    );
    assert_eq!(
      motion(modes(MouseMode::Motion), 1, 1, None),
      Some(vec![0x1b, b'[', b'M', 67, 34, 34])
    );
  }

  #[test]
  fn big_coordinates() {
    let m = modes(MouseMode::Press);
    assert_eq!(button(m, 224, 0, 0, true), None);
    let utf8 = MouseModes { utf8: true, ..m };
    // 32 + 300 + 1 is U+014D, two bytes.
    assert_eq!(button(utf8, 300, 0, 0, true), Some("\x1b[M \u{14d}!".as_bytes().to_vec()));
  }

  #[test]
  fn focus_events() {
    assert_eq!(focus(MouseModes::default(), true), None);
    let m = MouseModes { focus: true, ..Default::default() };
    assert_eq!(focus(m, true), Some(b"\x1b[I".to_vec()));
    assert_eq!(focus(m, false), Some(b"\x1b[O".to_vec()));
  }
}
