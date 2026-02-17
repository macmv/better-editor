use std::{collections::HashSet, fmt};

pub use toml::de::{DeTable, DeValue};

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

pub(crate) struct Parser {
  allow_partial: bool,
  diagnostics:   Vec<Diagnostic>,
}

pub(crate) trait ParseTable {
  /// Returns all keys that are required.
  fn required_keys() -> &'static [&'static str];
  /// Sets the key from a table entry. Returns `true` if the struct was
  /// modified, and `false` if the struct does not respond to the given key.
  fn set_key(&mut self, key: &str, value: DeValue, de: &mut Parser) -> bool;
}

pub(crate) trait ParseValue
where
  Self: Sized,
{
  fn parse(&mut self, value: DeValue, de: &mut Parser) -> Result<(), String>;
}

impl<T> ParseValue for T
where
  T: Default + ParseTable,
{
  fn parse(&mut self, value: DeValue, de: &mut Parser) -> Result<(), String> {
    match value {
      DeValue::Table(table) => {
        de.table(self, table);
        Ok(())
      }
      _ => Err("expected table".to_string()),
    }
  }
}

pub fn parse<T: Default + ParseTable>(content: &str) -> ParseResult<T> {
  let mut parser = Parser { allow_partial: false, diagnostics: vec![] };

  let mut value = T::default();
  if let Some(table) = parser.check(DeTable::parse(content)) {
    parser.table(&mut value, table.into_inner())
  };

  ParseResult { value, diagnostics: parser.diagnostics }
}

pub fn parse_into<T: Default + ParseTable>(value: &mut T, content: &str) -> Vec<Diagnostic> {
  let mut parser = Parser { allow_partial: true, diagnostics: vec![] };

  if let Some(table) = parser.check(DeTable::parse(content)) {
    parser.table(value, table.into_inner())
  };

  parser.diagnostics
}

impl Parser {
  pub fn table<T: Default + ParseTable>(&mut self, out: &mut T, table: DeTable) {
    let mut required = if self.allow_partial {
      None
    } else {
      Some(HashSet::<&str>::from_iter(T::required_keys().iter().copied()))
    };

    for (k, v) in table {
      if let Some(required) = &mut required {
        required.remove(&**k.get_ref());
      }

      if !out.set_key(k.get_ref(), v.into_inner(), self) {
        self.warn(format!("unknown key: {}", k.get_ref()), k.span());
      }
    }

    if let Some(required) = required {
      for key in required {
        self.error(format!("missing key: '{}'", key), 0..0); // todo: bah this library is bad
      }
    }
  }

  pub fn complete_value<T: Default + ParseValue>(&mut self, value: DeValue) -> T {
    let mut v = T::default();
    let partial = self.allow_partial;
    self.allow_partial = false;
    self.partial_value(&mut v, value);
    self.allow_partial = partial;
    v
  }

  pub fn partial_value<T: Default + ParseValue>(&mut self, v: &mut T, value: DeValue) {
    let res = v.parse(value, self);
    self.check(res).unwrap_or_default()
  }

  fn check<U, E: std::fmt::Display>(&mut self, result: Result<U, E>) -> Option<U> {
    match result {
      Ok(value) => Some(value),
      Err(err) => {
        self.diagnostics.push(Diagnostic {
          title: err.to_string(),
          line:  0,
          level: DiagnosticLevel::Error,
        });
        None
      }
    }
  }

  pub fn error(&mut self, title: String, span: std::ops::Range<usize>) {
    self.diagnostics.push(Diagnostic {
      title,
      line: span.start as u32,
      level: DiagnosticLevel::Error,
    })
  }

  pub fn warn(&mut self, title: String, span: std::ops::Range<usize>) {
    self.diagnostics.push(Diagnostic {
      title,
      line: span.start as u32,
      level: DiagnosticLevel::Warning,
    })
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

macro_rules! int {
  ($($ty:ty)*) => {
    $(
    impl ParseValue for $ty {
      fn parse(&mut self, value: DeValue, _de: &mut Parser) -> Result<(), String> {
        match value {
          DeValue::Integer(i) => {
            *self = <$ty>::from_str_radix(i.as_str(), i.radix()).map_err(|_| "expected integer".to_string())?;
            Ok(())
          }
          _ => Err("expected integer".to_string()),
        }
      }
    }
    )*
  };
}

int!(i8 i16 i32 i64 u8 u16 u32 u64 isize usize);

impl ParseValue for f32 {
  fn parse(&mut self, value: DeValue, _de: &mut Parser) -> Result<(), String> {
    *self = match value {
      DeValue::Integer(i) => i.as_str().parse().map_err(|_| "expected float".to_string())?,
      DeValue::Float(i) => i.as_str().parse().map_err(|_| "expected float".to_string())?,
      _ => return Err("expected float".to_string()),
    };

    Ok(())
  }
}

impl ParseValue for f64 {
  fn parse(&mut self, value: DeValue, _de: &mut Parser) -> Result<(), String> {
    *self = match value {
      DeValue::Integer(i) => i.as_str().parse().map_err(|_| "expected float".to_string())?,
      DeValue::Float(i) => i.as_str().parse().map_err(|_| "expected float".to_string())?,
      _ => return Err("expected float".to_string()),
    };

    Ok(())
  }
}

impl ParseValue for String {
  fn parse(&mut self, value: DeValue, _de: &mut Parser) -> Result<(), String> {
    match value {
      DeValue::String(s) => *self = s.into(),
      _ => return Err("expected string".to_string()),
    }

    Ok(())
  }
}

impl<T: ParseValue + Default> ParseValue for Vec<T> {
  fn parse(&mut self, value: DeValue, de: &mut Parser) -> Result<(), String> {
    // NB: Parsing arrays replaces them.
    self.clear();

    match value {
      DeValue::Array(a) => self.extend(a.into_iter().map(|it| de.complete_value(it.into_inner()))),
      _ => return Err("expected array".to_string()),
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use be_config_macros::Config;
  use expect_test::{Expect, expect};

  use super::*;

  fn fmt_diags(diags: &[Diagnostic]) -> String {
    let mut lines = diags
      .iter()
      .map(|d| {
        let level = match d.level {
          DiagnosticLevel::Error => "error",
          DiagnosticLevel::Warning => "warning",
        };
        format!("{level}: {}", d.title)
      })
      .collect::<Vec<_>>();
    lines.sort();
    lines.join("\n")
  }

  fn render<T: std::fmt::Debug>(value: &T, diags: &[Diagnostic]) -> String {
    let mut content = format!("{:#?}", value).replace("    ", "  ");
    if !diags.is_empty() {
      content.push_str("\n\n");
      content.push_str(&fmt_diags(diags));
    }

    content.push('\n');

    content
  }

  fn check_parse<T>(content: &str, expect: Expect)
  where
    T: Default + ParseTable + std::fmt::Debug,
  {
    let res = parse::<T>(content);
    expect.assert_eq(&render(&res.value, &res.diagnostics));
  }

  fn check_parse_chain<T>(base: &str, overlay: &str, expect: Expect)
  where
    T: Default + ParseTable + std::fmt::Debug,
  {
    let mut res = parse::<T>(base);
    let mut diagnostics = res.diagnostics;
    diagnostics.extend(parse_into(&mut res.value, overlay));
    expect.assert_eq(&render(&res.value, &diagnostics));
  }

  #[derive(Default, Debug, Config)]
  struct Plain {
    n:     i32,
    label: String,
  }

  #[test]
  fn parse_plain_happy() {
    check_parse::<Plain>(
      r#"
      n = 2
      label = "ok"
      "#,
      expect![@r#"
        Plain {
          n: 2,
          label: "ok",
        }
      "#],
    );
  }

  #[test]
  fn parse_plain_missing_and_unknown() {
    check_parse::<Plain>(
      r#"
      n = 1
      extra = 7
      "#,
      expect![@r#"
        Plain {
          n: 1,
          label: "",
        }

        error: missing key: 'label'
        warning: unknown key: extra
      "#],
    );
  }

  #[derive(Default, Debug, Config)]
  struct Doc {
    leaf:  Leaf,
    mode:  Mode,
    node:  Node,
    items: Vec<Item>,
  }
  #[allow(dead_code)]
  #[derive(Default, Debug, Config)]
  #[config(tag = "kind")]
  enum Node {
    #[default]
    None,
    Root,
    Leaf(Leaf),
  }
  #[derive(Default, Debug, Config)]
  struct Leaf {
    size: u32,
  }
  #[derive(Default, Debug, Config)]
  struct Item {
    value: i32,
  }
  #[derive(Default, Debug, Config)]
  enum Mode {
    #[default]
    Alpha,
    Beta,
  }

  #[test]
  fn parse_doc_happy() {
    check_parse::<Doc>(
      r#"
      mode = "beta"

      [leaf]
      size = 4

      [node]
      kind = "root"

      [[items]]
      value = 1
      "#,
      expect![@r#"
        Doc {
          leaf: Leaf {
            size: 4,
          },
          mode: Beta,
          node: Root,
          items: [
            Item {
              value: 1,
            },
          ],
        }
      "#],
    );
  }

  #[test]
  fn chain_overrides_and_partials() {
    check_parse_chain::<Doc>(
      r#"
      mode = "alpha"

      [leaf]
      size = 3

      [node]
      kind = "none"

      [[items]]
      value = 10

      [[items]]
      value = 20
      "#,
      r#"
      mode = "beta"

      [node]
      kind = "leaf"
      size = 3

      [leaf]
      size = 9
      "#,
      expect![@r#"
        Doc {
          leaf: Leaf {
            size: 9,
          },
          mode: Beta,
          node: Leaf(
            Leaf {
              size: 3,
            },
          ),
          items: [
            Item {
              value: 10,
            },
            Item {
              value: 20,
            },
          ],
        }
      "#],
    );
  }

  #[test]
  fn chain_array_replaces() {
    check_parse_chain::<Doc>(
      r#"
      mode = "alpha"

      [leaf]
      size = 3

      [node]
      kind = "none"

      [[items]]
      value = 10

      [[items]]
      value = 20
      "#,
      r#"
      [[items]]
      value = 99
      "#,
      expect![@r#"
        Doc {
          leaf: Leaf {
            size: 3,
          },
          mode: Alpha,
          node: None,
          items: [
            Item {
              value: 99,
            },
          ],
        }
      "#],
    );
  }

  #[derive(Default, Debug, Config)]
  struct SingleNode {
    node: Node,
  }

  #[test]
  fn tagged_enum_paths() {
    check_parse_chain::<SingleNode>(
      r#"
      [node]
      kind = "none"
      "#,
      r#"
      [node]
      kind = "root"
      unused = 1
      "#,
      expect![@r#"
        SingleNode {
          node: Root,
        }

        warning: unknown key for variant 'root'
      "#],
    );

    check_parse_chain::<SingleNode>(
      r#"
      [node]
      kind = "none"
      "#,
      r#"
      [node]
      kind = "leaf"
      "#,
      expect![@r#"
        SingleNode {
          node: Leaf(
            Leaf {
              size: 0,
            },
          ),
        }

        error: missing key: 'size'
      "#],
    );
  }

  #[test]
  fn tagged_enum_errors_and_unknown_top_level() {
    check_parse_chain::<SingleNode>(
      r#"
      [node]
      kind = "none"
      "#,
      r#"
      [node]
      value = 1
      "#,
      expect![@r#"
        SingleNode {
          node: None,
        }

        error: missing key: 'kind'
      "#],
    );

    check_parse_chain::<SingleNode>(
      r#"
      [node]
      kind = "none"
      "#,
      r#"
      [node]
      kind = "wat"
      "#,
      expect![@r#"
        SingleNode {
          node: None,
        }

        error: unknown kind variant: 'wat'
      "#],
    );

    check_parse_chain::<SingleNode>(
      r#"
      [node]
      kind = "none"
      "#,
      r#"
      mystery = 1
      "#,
      expect![@r#"
        SingleNode {
          node: None,
        }

        warning: unknown key: mystery
      "#],
    );
  }
}
