use anstyle_parse::{Params, Perform};

use crate::{BuiltinColor, Charset, Style, StyleFlags, TerminalColor, TerminalState};

impl Perform for TerminalState {
  fn print(&mut self, c: char) {
    if self.cursor.insert {
      self.grid.line_mut(self.cursor.row).shift_right_from(self.cursor.pos.col);
    }

    self.grid.put(
      self.cursor.pos,
      self.charsets[self.cursor.active_charset].map(c),
      self.cursor.style,
    );
    self.cursor.col += 1;
  }

  fn execute(&mut self, b: u8) {
    match b {
      C0::BS => self.cursor.col = self.cursor.col.saturating_sub(1),
      C0::CR => self.cursor.col = 0,
      C0::LF | C0::VT | C0::FF => {
        self.linefeed();
        if self.cursor.line_feed {
          self.cursor.col = 0;
        }
      }
      C0::BEL => {} // Ignore bell.
      C0::SI => self.set_active_charset(0),
      C0::SO => self.set_active_charset(1),
      _ => debug!("[unhandled C0] {b}"),
    }
  }

  fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
    macro_rules! unhandled {
      () => {{
        debug!("[unhandled ESC] byte={byte:?} intermediates={intermediates:?}");
      }};

      ($msg:literal) => {{
        debug!("[unhandled ESC] {} (byte={byte:?} intermediates={intermediates:?})", $msg);
      }};
    }

    match (byte, intermediates) {
      (b'B', &[index]) => self.set_charset(index, Charset::Ascii),
      (b'D', []) => self.linefeed(),
      (b'E', []) => {
        self.linefeed();
        self.cursor.col = 0;
      }
      (b'H', []) => unhandled!("set horizontal tab stop"),
      (b'M', []) => {
        if self.cursor.row == self.scroll_start {
          self.grid.scroll_down(self.scroll_start..self.scroll_end);
        } else {
          self.cursor.row = self.cursor.row.saturating_sub(1);
        }
      }
      (b'Z', []) => self.identify_terminal(None),
      (b'c', []) => unhandled!("reset state"),
      (b'g', []) => {} // Visual bell, ignore.
      (b'0', &[index]) => self.set_charset(index, Charset::LineDrawing),
      (b'7', []) => unhandled!("save cursor position"),
      (b'8', [b'#']) => unhandled!("show test screen"),
      (b'8', []) => unhandled!("restore cursor position"),
      (b'=', []) => self.keypad_application_mode = true,
      (b'>', []) => self.keypad_application_mode = false,
      // String terminator, do nothing (parser handles as string terminator).
      (b'\\', []) => (),
      _ => unhandled!(),
    }
  }

  fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
    macro_rules! unhandled {
      () => {{
        debug!("[unhandled OSC] params={params:?}");
      }};

      ($msg:literal) => {{
        debug!("[unhandled OSC] {} (params={params:?})", $msg);
      }};
    }

    match params[0] {
      b"0" | b"2" => {
        let title = params[1..]
          .iter()
          .flat_map(|x| str::from_utf8(x))
          .collect::<Vec<&str>>()
          .join(";")
          .trim()
          .to_owned();

        self.title = title;
      }
      b"4" => unhandled!("set color index"),
      b"8" if params.len() > 2 => unhandled!("hyperline"),
      b"10" | b"11" | b"12" => unhandled!("set color"),
      b"22" if params.len() == 2 => unhandled!("set cursor shape"),
      b"50" => unhandled!("set cursor style"),
      b"52" => unhandled!("set clipboard"),
      b"104" => unhandled!("reset color index"),
      b"110" => unhandled!("reset foreground color"),
      b"111" => unhandled!("reset background color"),
      b"112" => unhandled!("reset cursor color"),

      _ => unhandled!(),
    }
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
        debug!(
          "[unhandled CSI] action={action:?}, params={params:?}, intermediates={intermediates:?}",
        );
      }};

      ($($msg:tt)*) => {{
        debug!(
          "[unhandled CSI] {} (action={action:?}, params={params:?}, intermediates={intermediates:?})",
          format_args!($($msg)*)
        );
      }};
    }

    let mut params_iter = params.iter();

    let mut next_param_or = |default: u16| match params_iter.next() {
      Some(&[param, ..]) if param != 0 => param,
      _ => default,
    };

    match (action, intermediates) {
      (b'@', []) => {
        self.grid.line_mut(self.cursor.row).shift_right_from(self.cursor.col);
        self.grid.put(self.cursor.pos, ' ', self.cursor.style);
      }
      (b'A', []) => self.move_up(next_param_or(1)),
      (b'B', []) | (b'e', []) => self.move_down(next_param_or(1)),
      (b'b', []) => unhandled!("repeat the preceding char"),
      (b'C', []) | (b'a', []) => self.move_right(next_param_or(1)),
      (b'c', intermediates) if next_param_or(0) == 0 => {
        self.identify_terminal(intermediates.first().copied())
      }
      (b'D', []) => self.move_left(next_param_or(1)),
      (b'd', []) => {
        self.cursor.row = (next_param_or(1) as usize - 1).clamp(0, self.size.rows - 1);
      }
      (b'E', []) => {
        self.move_down(next_param_or(1));
        self.cursor.col = 0;
      }
      (b'F', []) => {
        self.move_up(next_param_or(1));
        self.cursor.col = 0;
      }
      (b'G', []) | (b'`', []) => {
        self.cursor.col = (next_param_or(1) as usize - 1).clamp(0, self.size.cols - 1);
      }
      (b'W', [b'?']) if next_param_or(0) == 5 => unhandled!("set tabs to 8"),
      (b'g', []) => unhandled!("clear tabs"),
      (b'H', []) | (b'f', []) => {
        self.cursor.row = (next_param_or(1) as usize - 1).clamp(0, self.size.rows - 1);
        self.cursor.col = (next_param_or(1) as usize - 1).clamp(0, self.size.cols - 1);
      }
      (b'h', []) => {
        for param in params_iter.map(|param| param[0]) {
          self.set_mode(param, true);
        }
      }
      (b'h', [b'?']) => {
        for param in params_iter.map(|param| param[0]) {
          self.set_private_mode(param, true)
        }
      }
      (b'I', []) => unhandled!("move forward tabs"),
      (b'J', []) => match next_param_or(0) {
        0 => self.clear_screen_down(),
        1 => self.clear_screen_up(),
        2 => self.grid.clear(self.cursor.style),
        3 => {
          self.grid.clear(self.cursor.style);
          self.scrollback.clear();
        }
        param => unhandled!("clear screen with {}", param),
      },
      (b'K', []) => match next_param_or(0) {
        0 => self.clear_line_right(),
        1 => self.clear_line_left(),
        2 => self.clear_line(),
        param => unhandled!("clear line with {}", param),
      },
      (b'k', [b' ']) => unhandled!("set scp"),
      (b'L', []) => unhandled!("insert blank lines"),
      (b'l', []) => {
        for param in params_iter.map(|param| param[0]) {
          self.set_mode(param, false);
        }
      }
      (b'l', [b'?']) => {
        for param in params_iter.map(|param| param[0]) {
          self.set_private_mode(param, false)
        }
      }
      (b'M', []) => unhandled!("delete lines"),
      (b'm', []) => self.set_graphics_mode(params),
      (b'm', [b'>']) => unhandled!("set keyboard mode"),
      (b'm', [b'?']) => unhandled!("report graphics mode"),
      (b'n', []) => {
        match next_param_or(0) {
          // "is the device functioning?" -> "yes"
          5 => self.send_text("\x1b[0n"),
          // "where is the cursor"
          6 => {
            self.send_text(&format!("\x1b[{};{}R", self.cursor.row + 1, self.cursor.col + 1));
          }
          arg => unhandled!("unknown device status query: {arg}"),
        };
      }
      (b'P', []) => unhandled!("delete chars"),
      (b'p', [b'$']) => unhandled!("report mode"),
      (b'p', [b'?', b'$']) => unhandled!("report private mode"),
      (b'q', [b' ']) => unhandled!("set cursor style"),
      (b'r', []) => {
        let scroll_start = (next_param_or(1) as usize - 1).clamp(0, self.size.rows - 1);
        let scroll_end = (next_param_or(self.size.rows as u16) as usize).clamp(0, self.size.rows);
        if scroll_start < scroll_end {
          self.scroll_start = scroll_start;
          self.scroll_end = scroll_end;
        }
        self.cursor.row = 0;
        self.cursor.col = 0;
      }
      (b'S', []) => {
        for _ in 0..next_param_or(1) {
          self.grid.scroll_up(self.scroll_start..self.scroll_end);
        }
      }
      (b's', []) => unhandled!("save cursor position"),
      (b'T', []) => {
        for _ in 0..next_param_or(1) {
          self.grid.scroll_down(self.scroll_start..self.scroll_end);
        }
      }
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

impl TerminalState {
  fn move_up(&mut self, n: u16) { self.cursor.row = self.cursor.row.saturating_sub(n as usize); }
  fn move_down(&mut self, n: u16) {
    self.cursor.row = (self.cursor.row + n as usize).clamp(0, self.size.rows - 1);
  }
  fn move_left(&mut self, n: u16) { self.cursor.col = self.cursor.col.saturating_sub(n as usize); }
  fn move_right(&mut self, n: u16) {
    self.cursor.col = (self.cursor.col + n as usize).clamp(0, self.size.cols - 1);
  }

  fn clear_screen_down(&mut self) {
    for line in self.cursor.row..=self.size.rows - 1 {
      self.grid.line_mut(line).clear(self.cursor.style);
    }
  }

  fn clear_screen_up(&mut self) {
    for line in 0..=self.cursor.row {
      self.grid.line_mut(line).clear(self.cursor.style);
    }
  }

  fn clear_line_right(&mut self) {
    self
      .grid
      .line_mut(self.cursor.row)
      .clear_range(self.cursor.col..=self.size.cols - 1, self.cursor.style);
  }

  fn clear_line_left(&mut self) {
    self.grid.line_mut(self.cursor.row).clear_range(0..=self.cursor.col, self.cursor.style);
  }

  fn clear_line(&mut self) {
    self.grid.line_mut(self.cursor.row).clear_range(0..=self.size.cols - 1, self.cursor.style);
  }

  fn linefeed(&mut self) {
    if self.cursor.row == self.scroll_end - 1 {
      let line = self.grid.scroll_up(self.scroll_start..self.scroll_end);
      if !self.alt_screen {
        self.scrollback.push(line);
      }
    } else if self.cursor.row < self.size.rows - 1 {
      self.cursor.row += 1;
    }
  }

  fn send_text(&mut self, text: &str) { self.pending_writes.extend_from_slice(text.as_bytes()); }

  fn set_charset(&mut self, index: u8, charset: Charset) {
    let index = match index {
      b'(' => 0,
      b')' => 1,
      b'*' => 2,
      b'+' => 3,
      _ => return,
    };
    self.charsets[index] = charset;
  }

  fn set_active_charset(&mut self, index: usize) { self.cursor.active_charset = index; }

  fn set_mode(&mut self, mode: u16, set: bool) {
    macro_rules! unhandled {
      ($mode:literal) => {
        debug!("[unhandled mode] {mode} ({})", $mode)
      };
    }

    match mode {
      4 => self.cursor.insert = set,
      20 => self.cursor.line_feed = set,
      34 => self.cursor.blink = set,
      _ => unhandled!("unknown"),
    }
  }

  fn identify_terminal(&mut self, arg: Option<u8>) {
    match arg {
      // primary device attributes
      None => self.send_text("\x1b[?6c"),
      // secondary device attributes
      Some(b'>') => self.send_text("\x1b[>0;1;1c"),

      _ => debug!("[unhandled identify terminal] arg={arg:?}"),
    }
  }

  fn set_private_mode(&mut self, mode: u16, set: bool) {
    macro_rules! unhandled {
      ($mode:literal) => {
        debug!("[unhandled private mode] {mode} ({})", $mode)
      };
    }

    match mode {
      1 => self.cursor_keys = set,
      3 => unhandled!("column mode"),
      6 => unhandled!("origin"),
      7 => unhandled!("line wrap"),
      12 => self.cursor.blink = set,
      25 => self.cursor.visible = !set,
      1000 => self.report_mouse = set,
      1002 => unhandled!("report cell mouse motion"),
      1003 => unhandled!("report all mouse motion"),
      1004 => unhandled!("report focus in out"),
      1005 => unhandled!("utf8 mouse"),
      1006 => unhandled!("sgr mouse"),
      1007 => unhandled!("alternate scroll"),
      1042 => unhandled!("urgency hints"),
      1049 => self.set_alt_screen(set),
      2004 => self.bracketed_paste = set,
      2026 => unhandled!("sync update"),
      _ => unhandled!("unknown"),
    }
  }

  fn set_alt_screen(&mut self, set: bool) {
    if set == self.alt_screen {
      return;
    }

    self.alt_screen = set;
    std::mem::swap(&mut self.grid, &mut self.alt_grid);

    if self.alt_screen {
      self.alt_cursor = self.cursor;
    } else {
      self.cursor = self.alt_cursor;
      self.alt_grid.clear(Default::default());
      self.alt_cursor = Default::default();
    }
  }

  fn set_graphics_mode(&mut self, params: &Params) {
    macro_rules! builtin {
      ($name:ident, $bright:expr) => {
        TerminalColor::Builtin { color: BuiltinColor::$name, bright: $bright }
      };
    }

    let style = &mut self.cursor.style;
    let mut iter = params.iter();

    while let Some(args) = iter.next() {
      match args {
        [0] => *style = Style::default(),
        [1] => style.flags.set(StyleFlags::BOLD, true),
        [2] => style.flags.set(StyleFlags::DIM, true),
        [3] => style.flags.set(StyleFlags::ITALIC, true),
        [4] => style.flags.set(StyleFlags::UNDERLINE, true),
        [5] => style.flags.set(StyleFlags::BLINK, true),
        [7] => style.flags.set(StyleFlags::INVERSE, true),
        [8] => style.flags.set(StyleFlags::HIDDEN, true),
        [9] => style.flags.set(StyleFlags::STRIKETHROUGH, true),

        [22] => style.flags.set(StyleFlags::BOLD | StyleFlags::DIM, false),
        [23] => style.flags.set(StyleFlags::ITALIC, false),
        [24] => style.flags.set(StyleFlags::UNDERLINE, false),
        [25] => style.flags.set(StyleFlags::BLINK, false),
        [27] => style.flags.set(StyleFlags::INVERSE, false),
        [28] => style.flags.set(StyleFlags::HIDDEN, false),
        [29] => style.flags.set(StyleFlags::STRIKETHROUGH, false),

        [30] => style.foreground = Some(builtin!(Black, false)),
        [31] => style.foreground = Some(builtin!(Red, false)),
        [32] => style.foreground = Some(builtin!(Green, false)),
        [33] => style.foreground = Some(builtin!(Yellow, false)),
        [34] => style.foreground = Some(builtin!(Blue, false)),
        [35] => style.foreground = Some(builtin!(Magenta, false)),
        [36] => style.foreground = Some(builtin!(Cyan, false)),
        [37] => style.foreground = Some(builtin!(White, false)),
        [38] => {
          if let Some(color) = parse_color((&mut iter).map(|param| param[0])) {
            style.foreground = Some(color);
          }
        }
        [38, params @ ..] => {
          if let Some(color) = parse_color(params.iter().copied()) {
            style.foreground = Some(color);
          }
        }
        [39] => style.foreground = None,

        [40] => style.background = Some(builtin!(Black, false)),
        [41] => style.background = Some(builtin!(Red, false)),
        [42] => style.background = Some(builtin!(Green, false)),
        [43] => style.background = Some(builtin!(Yellow, false)),
        [44] => style.background = Some(builtin!(Blue, false)),
        [45] => style.background = Some(builtin!(Magenta, false)),
        [46] => style.background = Some(builtin!(Cyan, false)),
        [47] => style.background = Some(builtin!(White, false)),
        [48] => {
          if let Some(color) = parse_color((&mut iter).map(|param| param[0])) {
            style.background = Some(color);
          }
        }
        [48, params @ ..] => {
          if let Some(color) = parse_color(params.iter().copied()) {
            style.background = Some(color);
          }
        }
        [49] => style.background = None,

        [90] => style.foreground = Some(builtin!(Black, true)),
        [91] => style.foreground = Some(builtin!(Red, true)),
        [92] => style.foreground = Some(builtin!(Green, true)),
        [93] => style.foreground = Some(builtin!(Yellow, true)),
        [94] => style.foreground = Some(builtin!(Blue, true)),
        [95] => style.foreground = Some(builtin!(Magenta, true)),
        [96] => style.foreground = Some(builtin!(Cyan, true)),
        [97] => style.foreground = Some(builtin!(White, true)),

        [100] => style.background = Some(builtin!(Black, true)),
        [101] => style.background = Some(builtin!(Red, true)),
        [102] => style.background = Some(builtin!(Green, true)),
        [103] => style.background = Some(builtin!(Yellow, true)),
        [104] => style.background = Some(builtin!(Blue, true)),
        [105] => style.background = Some(builtin!(Magenta, true)),
        [106] => style.background = Some(builtin!(Cyan, true)),
        [107] => style.background = Some(builtin!(White, true)),

        _ => {
          debug!("unhandle graphics mode: {args:?}");
        }
      }
    }
  }
}

fn parse_color(mut iter: impl Iterator<Item = u16>) -> Option<TerminalColor> {
  match iter.next() {
    Some(2) => Some(TerminalColor::Rgb {
      r: iter.next()? as u8,
      g: iter.next()? as u8,
      b: iter.next()? as u8,
    }),
    Some(5) => None, // TODO: Indexed colors.

    _ => None,
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
