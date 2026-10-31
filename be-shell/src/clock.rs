//! Wall-clock times for display, in the local time zone.

use std::time::{SystemTime, UNIX_EPOCH};

/// `2026-10-30 22:54:57` in the local time zone. If the zone can't be
/// determined, the time is in UTC and ends in ` UTC`.
pub fn format_local_time(time: SystemTime) -> String {
  let secs = match time.duration_since(UNIX_EPOCH) {
    Ok(d) => d.as_secs() as i64,
    Err(e) => -(e.duration().as_secs() as i64),
  };
  match local_offset(secs) {
    Some(offset) => format_civil(secs, offset),
    None => format!("{} UTC", format_civil(secs, 0)),
  }
}

/// The offset of the local time zone from UTC, in seconds, at `unix_secs`.
fn local_offset(unix_secs: i64) -> Option<i64> {
  let t = unix_secs as libc::time_t;
  // SAFETY: `tm` is plain data, and `localtime_r` only writes through the
  // pointers given to it.
  unsafe {
    let mut tm: libc::tm = std::mem::zeroed();
    if libc::localtime_r(&t, &mut tm).is_null() {
      return None;
    }
    Some(tm.tm_gmtoff as i64)
  }
}

/// `YYYY-MM-DD HH:MM:SS` for `unix_secs` seen from a zone `offset` seconds
/// east of UTC.
pub fn format_civil(unix_secs: i64, offset: i64) -> String {
  let t = unix_secs + offset;
  let (days, rem) = (t.div_euclid(86400), t.rem_euclid(86400));
  let (y, m, d) = civil_from_days(days);
  format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

/// The proleptic Gregorian date of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
  let z = days + 719468;
  let era = z.div_euclid(146097);
  let doe = z.rem_euclid(146097);
  let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let m = if mp < 10 { mp + 3 } else { mp - 9 };
  (yoe + era * 400 + (m <= 2) as i64, m, d)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn formats_with_an_offset() {
    assert_eq!(format_civil(0, 0), "1970-01-01 00:00:00");
    assert_eq!(format_civil(1_000_000_000, 0), "2001-09-09 01:46:40");
    // Eight hours west: the previous day.
    assert_eq!(format_civil(1_000_000_000, -8 * 3600), "2001-09-08 17:46:40");
    assert_eq!(format_civil(951_782_400, 0), "2000-02-29 00:00:00");
    assert_eq!(format_civil(951_782_400 + 86400, 5 * 3600 + 1800), "2000-03-01 05:30:00");
    assert_eq!(format_civil(-1, 0), "1969-12-31 23:59:59");
  }
}
