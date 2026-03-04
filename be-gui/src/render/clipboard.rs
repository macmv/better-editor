use be_input::{Clipboard, ClipboardBackend};
use winit::window::Window;

struct WaylandClipboard(smithay_clipboard::Clipboard);

pub fn create(window: &Window) -> Clipboard {
  use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

  match window.display_handle().unwrap().as_raw() {
    RawDisplayHandle::Wayland(wl) => Clipboard::new(WaylandClipboard(unsafe {
      smithay_clipboard::Clipboard::new(wl.display.as_ptr() as *mut _)
    })),

    handle => {
      error!("clipboard not implemented for display handle {handle:?}");
      Clipboard::dummy()
    }
  }
}

impl ClipboardBackend for WaylandClipboard {
  fn paste(&self) -> String {
    self.0.load().unwrap_or_else(|e| {
      error!("failed to paste: {e}");
      String::new()
    })
  }

  fn copy(&self, content: &str) { self.0.store(content); }
}
