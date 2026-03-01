use winit::window::Window;

pub struct Clipboard {
  imp: ClipboardImpl,
}

#[allow(dead_code)]
enum ClipboardImpl {
  Wayland(smithay_clipboard::Clipboard),

  Dummy,
}

impl Clipboard {
  pub fn new(window: &Window) -> Self {
    use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    let imp = match window.display_handle().unwrap().as_raw() {
      RawDisplayHandle::Wayland(wl) => unsafe {
        ClipboardImpl::Wayland(smithay_clipboard::Clipboard::new(wl.display.as_ptr() as *mut _))
      },

      handle => {
        error!("clipboard not implemented for display handle {handle:?}");
        ClipboardImpl::Dummy
      }
    };

    Clipboard { imp }
  }

  pub fn copy(&self, content: &str) { self.imp.copy(content) }
  pub fn paste(&self) -> String { self.imp.paste() }
}

impl ClipboardImpl {
  fn paste(&self) -> String {
    match self {
      ClipboardImpl::Wayland(c) => c.load().unwrap_or_else(|e| {
        error!("failed to paste: {e}");
        String::new()
      }),
      ClipboardImpl::Dummy => String::new(),
    }
  }

  fn copy(&self, content: &str) {
    match self {
      ClipboardImpl::Wayland(c) => c.store(content),
      ClipboardImpl::Dummy => {}
    }
  }
}
