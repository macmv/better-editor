use be_input::Clipboard;
use winit::window::Window;

#[cfg(target_os = "macos")]
use objc2_app_kit::NSPasteboard;
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;

#[cfg(target_os = "linux")]
struct WaylandClipboard(smithay_clipboard::Clipboard);

#[cfg(target_os = "macos")]
struct AppKitClipboard;

pub fn create(window: &Window) -> Clipboard {
  use winit::raw_window_handle::HasDisplayHandle;

  match window.display_handle().unwrap().as_raw() {
    #[cfg(target_os = "linux")]
    winit::raw_window_handle::RawDisplayHandle::Wayland(wl) => {
      Clipboard::new(WaylandClipboard(unsafe {
        smithay_clipboard::Clipboard::new(wl.display.as_ptr() as *mut _)
      }))
    }

    #[cfg(target_os = "macos")]
    winit::raw_window_handle::RawDisplayHandle::AppKit(_) => Clipboard::new(AppKitClipboard),

    handle => {
      error!("clipboard not implemented for display handle {handle:?}");
      Clipboard::dummy()
    }
  }
}

#[cfg(target_os = "linux")]
impl be_input::ClipboardBackend for WaylandClipboard {
  fn paste(&self) -> String {
    self.0.load().unwrap_or_else(|e| {
      error!("failed to paste: {e}");
      String::new()
    })
  }

  fn copy(&self, content: &str) { self.0.store(content); }
}

#[cfg(target_os = "macos")]
impl be_input::ClipboardBackend for AppKitClipboard {
  fn paste(&self) -> String {
    let pasteboard = NSPasteboard::generalPasteboard();
    unsafe {
      if let Some(data) = pasteboard.dataForType(&objc2_app_kit::NSPasteboardTypeString) {
        String::from_utf8_lossy(data.as_bytes_unchecked()).to_string()
      } else {
        String::new()
      }
    }
  }

  fn copy(&self, content: &str) {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    let content = NSString::from_str(content);
    unsafe {
      pasteboard.setString_forType(&content, &objc2_app_kit::NSPasteboardTypeString);
    }
  }
}
