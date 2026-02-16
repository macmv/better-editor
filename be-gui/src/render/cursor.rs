use winit::window::Window;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CursorKind {
  Default,
  Pointer,

  ResizeEastWest,
  ResizeNorthSouth,
}

#[cfg(target_os = "macos")]
fn set_ns_cursor(kind: CursorKind) {
  use objc2::{class, msg_send, runtime::AnyObject};

  const FRAME_POS_LEFT: usize = 1 << 1;
  const FRAME_POS_TOP: usize = 1 << 0;
  const FRAME_DIR_ALL: usize = (1 << 0) | (1 << 1);

  unsafe {
    let ns_cursor = class!(NSCursor);
    let cursor: *mut AnyObject = match kind {
      CursorKind::Default => msg_send![ns_cursor, arrowCursor],
      CursorKind::Pointer => msg_send![ns_cursor, pointingHandCursor],
      CursorKind::ResizeEastWest => {
        msg_send![ns_cursor, frameResizeCursorFromPosition:FRAME_POS_LEFT, inDirections:FRAME_DIR_ALL]
      }
      CursorKind::ResizeNorthSouth => {
        msg_send![ns_cursor, frameResizeCursorFromPosition:FRAME_POS_TOP, inDirections:FRAME_DIR_ALL]
      }
    };

    let _: () = msg_send![cursor, set];
  }
}

pub fn set_cursor(_window: &Window, cursor: CursorKind) {
  #[cfg(target_os = "macos")]
  set_ns_cursor(cursor);

  #[cfg(not(target_os = "macos"))]
  _window.set_cursor(match cursor {
    CursorKind::Default => winit::window::CursorIcon::Default,
    CursorKind::Pointer => winit::window::CursorIcon::Pointer,
    CursorKind::ResizeEastWest => winit::window::CursorIcon::EwResize,
    CursorKind::ResizeNorthSouth => winit::window::CursorIcon::NsResize,
  });
}
