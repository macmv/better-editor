pub struct Clipboard {
  imp: Box<dyn ClipboardBackend>,
}

struct DummyClipboard;

impl Clipboard {
  pub fn new(imp: impl ClipboardBackend + 'static) -> Self { Clipboard { imp: Box::new(imp) } }
  pub fn dummy() -> Self { Clipboard { imp: Box::new(DummyClipboard) } }

  pub fn copy(&self, content: &str) { self.imp.copy(content) }
  pub fn paste(&self) -> String { self.imp.paste() }
}

pub trait ClipboardBackend {
  fn copy(&self, content: &str);
  fn paste(&self) -> String;
}

impl ClipboardBackend for DummyClipboard {
  fn copy(&self, _content: &str) {}
  fn paste(&self) -> String { String::new() }
}
