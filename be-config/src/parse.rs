use std::fmt;

pub struct ParseResult<T> {
  pub value:       T,
  pub diagnostics: Vec<Diagnostic>,
}

pub struct Diagnostic {
  pub title: String,
  pub line:  u32,
  pub level: DiagnosticLevel,
}

pub enum DiagnosticLevel {
  Error,
  Warning,
}

impl<T> ParseResult<T> {
  pub(crate) fn ok(value: T) -> Self { ParseResult { value, diagnostics: vec![] } }

  pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> ParseResult<U> {
    ParseResult { value: f(self.value), diagnostics: self.diagnostics }
  }
}

impl fmt::Display for Diagnostic {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "{}: {} at line {}",
      match self.level {
        DiagnosticLevel::Error => "error",
        DiagnosticLevel::Warning => "warning",
      },
      self.title,
      self.line
    )
  }
}
