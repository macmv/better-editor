use anstyle_parse::Perform;

use crate::TerminalState;

impl Perform for TerminalState {
  fn print(&mut self, c: char) {
    self.grid.put(self.cursor, c);
    self.cursor.col += 1;
  }

  fn execute(&mut self, b: u8) {
    match b {
      C0::BS => {}
      C0::CR => self.cursor.col = 0,
      C0::LF | C0::VT | C0::FF => self.cursor.row += 1,
      _ => (),
    }
  }

  fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
    macro_rules! unhandled {
      () => {{
        eprintln!("[unhandled ESC] byte={byte:?} intermediates={intermediates:?}");
      }};

      ($msg:literal) => {{
        eprintln!("[unhandled ESC] {} (byte={byte:?} intermediates={intermediates:?})", $msg);
      }};
    }

    unhandled!();
  }

  fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
    macro_rules! unhandled {
      () => {{
        eprintln!("[unhandled OSC] params={params:?}");
      }};

      ($msg:literal) => {{
        eprintln!("[unhandled OSC] {} (params={params:?})", $msg);
      }};
    }

    unhandled!();
  }

  fn csi_dispatch(
    &mut self,
    params: &anstyle_parse::Params,
    intermediates: &[u8],
    _ignore: bool,
    action: u8,
  ) {
    macro_rules! unhandled {
      () => {{
        eprintln!(
          "[unhandled CSI] action={action:?}, params={params:?}, intermediates={intermediates:?}",
        );
      }};

      ($msg:literal) => {{
        eprintln!(
          "[unhandled CSI] {} (action={action:?}, params={params:?}, intermediates={intermediates:?})",
          $msg
        );
      }};
    }

    let mut params_iter = params.iter();

    let mut next_param_or = |default: u16| match params_iter.next() {
      Some(&[param, ..]) if param != 0 => param,
      _ => default,
    };

    match (action, intermediates) {
      (b'@', []) => unhandled!("insert blank"),
      (b'A', []) => unhandled!("move up"),
      (b'B', []) | (b'e', []) => unhandled!("move down"),
      (b'b', []) => unhandled!("repeat the preceding char"),
      (b'C', []) | (b'a', []) => unhandled!("move forward"),
      (b'c', intermediates) if next_param_or(0) == 0 => unhandled!("identify terminal"),
      (b'D', []) => unhandled!("move left"),
      (b'd', []) => unhandled!("goto line"),
      (b'E', []) => unhandled!("move down and clear line"),
      (b'F', []) => unhandled!("move up and clear line"),
      (b'G', []) | (b'`', []) => unhandled!("goto column"),
      (b'W', [b'?']) if next_param_or(0) == 5 => unhandled!("set tabs to 8"),
      (b'g', []) => unhandled!("clear tabs"),
      (b'H', []) | (b'f', []) => unhandled!("goto `y`, `x`"),
      (b'h', []) => unhandled!("set mode"),
      (b'h', [b'?']) => unhandled!("set private mode"),
      (b'I', []) => unhandled!("move forward tabs"),
      (b'J', []) => unhandled!("clear screen (0: below, 1: above, 2: all, 3: saved)"),
      (b'K', []) => unhandled!("clear line (0: right, 1: left, 2: all)"),
      (b'k', [b' ']) => unhandled!("set scp"),
      (b'L', []) => unhandled!("insert blank lines"),
      (b'l', []) => unhandled!("reset mode"),
      (b'l', [b'?']) => unhandled!("reset private mode"),
      (b'M', []) => unhandled!("delete lines"),
      (b'm', []) => unhandled!("set graphics mode"),
      (b'm', [b'>']) => unhandled!("set keyboard mode"),
      (b'm', [b'?']) => unhandled!("report graphics mode"),
      (b'n', []) => unhandled!("device status"),
      (b'P', []) => unhandled!("delete chars"),
      (b'p', [b'$']) => unhandled!("report mode"),
      (b'p', [b'?', b'$']) => unhandled!("report private mode"),
      (b'q', [b' ']) => unhandled!("set cursor style"),
      (b'r', []) => unhandled!("set scrolling region"),
      (b'S', []) => unhandled!("scroll up"),
      (b's', []) => unhandled!("save cursor position"),
      (b'T', []) => unhandled!("scroll down"),
      (b't', []) => unhandled!("push title/text area"),
      (b'u', [b'?']) => unhandled!("report keyboard mode"),
      (b'u', [b'=']) => unhandled!("set keyboard mode"),
      (b'u', [b'>']) => unhandled!("push keyboard mode"),
      (b'u', [b'<']) => unhandled!("pop keyboard modes"),
      (b'u', []) => unhandled!("restore cursor position"),
      (b'X', []) => unhandled!("erase chars"),
      (b'Z', []) => unhandled!("move backward tabs"),
      _ => unhandled!(),
    }
  }
}

/// C0 set of 7-bit control characters (from ANSI X3.4-1977).
#[allow(unused, non_snake_case)]
pub mod C0 {
  /// Null filler, terminal should ignore this character.
  pub const NUL: u8 = 0x00;
  /// Start of Header.
  pub const SOH: u8 = 0x01;
  /// Start of Text, implied end of header.
  pub const STX: u8 = 0x02;
  /// End of Text, causes some terminal to respond with ACK or NAK.
  pub const ETX: u8 = 0x03;
  /// End of Transmission.
  pub const EOT: u8 = 0x04;
  /// Enquiry, causes terminal to send ANSWER-BACK ID.
  pub const ENQ: u8 = 0x05;
  /// Acknowledge, usually sent by terminal in response to ETX.
  pub const ACK: u8 = 0x06;
  /// Bell, triggers the bell, buzzer, or beeper on the terminal.
  pub const BEL: u8 = 0x07;
  /// Backspace, can be used to define overstruck characters.
  pub const BS: u8 = 0x08;
  /// Horizontal Tabulation, move to next predetermined position.
  pub const HT: u8 = 0x09;
  /// Linefeed, move to same position on next line (see also NL).
  pub const LF: u8 = 0x0A;
  /// Vertical Tabulation, move to next predetermined line.
  pub const VT: u8 = 0x0B;
  /// Form Feed, move to next form or page.
  pub const FF: u8 = 0x0C;
  /// Carriage Return, move to first character of current line.
  pub const CR: u8 = 0x0D;
  /// Shift Out, switch to G1 (other half of character set).
  pub const SO: u8 = 0x0E;
  /// Shift In, switch to G0 (normal half of character set).
  pub const SI: u8 = 0x0F;
  /// Data Link Escape, interpret next control character specially.
  pub const DLE: u8 = 0x10;
  /// (DC1) Terminal is allowed to resume transmitting.
  pub const XON: u8 = 0x11;
  /// Device Control 2, causes ASR-33 to activate paper-tape reader.
  pub const DC2: u8 = 0x12;
  /// (DC2) Terminal must pause and refrain from transmitting.
  pub const XOFF: u8 = 0x13;
  /// Device Control 4, causes ASR-33 to deactivate paper-tape reader.
  pub const DC4: u8 = 0x14;
  /// Negative Acknowledge, used sometimes with ETX and ACK.
  pub const NAK: u8 = 0x15;
  /// Synchronous Idle, used to maintain timing in Sync communication.
  pub const SYN: u8 = 0x16;
  /// End of Transmission block.
  pub const ETB: u8 = 0x17;
  /// Cancel (makes VT100 abort current escape sequence if any).
  pub const CAN: u8 = 0x18;
  /// End of Medium.
  pub const EM: u8 = 0x19;
  /// Substitute (VT100 uses this to display parity errors).
  pub const SUB: u8 = 0x1A;
  /// Prefix to an escape sequence.
  pub const ESC: u8 = 0x1B;
  /// File Separator.
  pub const FS: u8 = 0x1C;
  /// Group Separator.
  pub const GS: u8 = 0x1D;
  /// Record Separator (sent by VT132 in block-transfer mode).
  pub const RS: u8 = 0x1E;
  /// Unit Separator.
  pub const US: u8 = 0x1F;
  /// Delete, should be ignored by terminal.
  pub const DEL: u8 = 0x7F;
}
