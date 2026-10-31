//! How long a command took, for display.

use std::time::Duration;

/// Formats `d` the way Zim's duration-info module does (`3s`, `1m5s`,
/// `2h3m`), rounded to the nearest whole second. Milliseconds are never
/// shown. Returns `None` if it rounds to under a second.
pub fn format_duration(d: Duration) -> Option<String> {
  let total = d.as_secs_f64().round() as u64;
  if total == 0 {
    return None;
  }

  let (s, m, h) = (total % 60, (total / 60) % 60, total / 3600);
  Some(if h > 0 {
    format!("{h}h{m}m")
  } else if m > 0 {
    format!("{m}m{s}s")
  } else {
    format!("{s}s")
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fmt(ms: u64) -> Option<String> { format_duration(Duration::from_millis(ms)) }

  #[test]
  fn rounds_to_whole_seconds() {
    assert_eq!(fmt(0), None);
    assert_eq!(fmt(499), None);
    assert_eq!(fmt(500).as_deref(), Some("1s"));
    assert_eq!(fmt(1499).as_deref(), Some("1s"));
    assert_eq!(fmt(1500).as_deref(), Some("2s"));
    assert_eq!(fmt(2999).as_deref(), Some("3s"));
  }

  #[test]
  fn minutes_and_hours() {
    assert_eq!(fmt(59_600).as_deref(), Some("1m0s"));
    assert_eq!(fmt(65_000).as_deref(), Some("1m5s"));
    assert_eq!(fmt(3_599_000).as_deref(), Some("59m59s"));
    assert_eq!(fmt(3_600_000).as_deref(), Some("1h0m"));
    assert_eq!(fmt(7_385_000).as_deref(), Some("2h3m"));
  }
}
