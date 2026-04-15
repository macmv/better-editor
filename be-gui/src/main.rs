use std::{fs::OpenOptions, path::Path, time::SystemTime};

use fern::Dispatch;
use log::LevelFilter;

fn main() {
  let cache = be_config::cache_root().unwrap();
  std::fs::create_dir_all(&cache).unwrap();

  let session_time = chrono::Local::now();
  let log_filename = format!("session-{}.log", session_time.format("%Y-%m-%d-%H-%M-%S"));
  let log_path = cache.join(&log_filename);

  let file = OpenOptions::new().write(true).create(true).append(true).open(&log_path).unwrap();

  Dispatch::new()
    .format(|out, message, record| {
      out.finish(format_args!(
        "{time} [{level}] {module} {message}",
        time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        level = match record.level() {
          log::Level::Error => "\x1b[31mERROR\x1b[0m",
          log::Level::Warn => "\x1b[33mWARN\x1b[0m",
          log::Level::Info => "\x1b[32mINFO\x1b[0m",
          log::Level::Debug => "\x1b[34mDEBUG\x1b[0m",
          log::Level::Trace => "\x1b[35mTRACE\x1b[0m",
        },
        module = record.target(),
      ))
    })
    .level(LevelFilter::Debug)
    .level_for("naga", LevelFilter::Warn)
    .level_for("wgpu_core", LevelFilter::Warn)
    .level_for("wgpu_hal", LevelFilter::Warn)
    .level_for("sctk", LevelFilter::Warn)
    .level_for("vello", LevelFilter::Warn)
    .level_for("globset", LevelFilter::Warn)
    .level_for("ignore", LevelFilter::Warn)
    .level_for("tracing", LevelFilter::Warn)
    .chain(std::io::stdout())
    .chain(file)
    .apply()
    .unwrap();

  rotate_logs(&cache);

  let default_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |info| {
    let location =
      info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
    let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
      s.to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
      s.clone()
    } else {
      String::from("unknown panic payload")
    };
    log::error!("PANIC at {location}: {msg}");
    default_hook(info);
  }));

  let _s = if std::env::var("BE_PROFILE").is_ok() {
    puffin::set_scopes_on(true);
    Some(puffin_http::Server::new("127.0.0.1:8585").unwrap())
  } else {
    None
  };

  be_gui::run();
}

const TOTAL_LOGS: usize = 5;
const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

// Delete old session logs. Logs newer than 7 days are always kept. Logs older
// than 7 days are deleted unless fewer than 5 logs would remain total.
fn rotate_logs(cache: &Path) {
  let cutoff = SystemTime::now().checked_sub(MAX_AGE).unwrap_or(SystemTime::UNIX_EPOCH);

  let entries = match std::fs::read_dir(cache) {
    Ok(e) => e,
    Err(_) => return,
  };

  let mut logs: Vec<(SystemTime, std::path::PathBuf)> = entries
    .flatten()
    .filter(|e| {
      let name = e.file_name();
      let name = name.to_string_lossy();
      name.starts_with("session-") && name.ends_with(".log")
    })
    .filter_map(|e| {
      let modified = e.metadata().ok()?.modified().ok()?;
      Some((modified, e.path()))
    })
    .collect();

  // Newest first.
  logs.sort_by(|a, b| b.0.cmp(&a.0));

  // Delete logs if:
  // - There are more than TOTAL_LOGS logs.
  // - The log file is older than MAX_AGE days.
  let mut total = 0;
  for (modified, path) in logs.into_iter() {
    if total >= TOTAL_LOGS && modified < cutoff {
      let _ = std::fs::remove_file(path);
    } else {
      total += 1;
    }
  }
}
