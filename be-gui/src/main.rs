use std::fs::OpenOptions;

use fern::Dispatch;
use log::LevelFilter;

fn main() {
  let cache = be_config::cache_root().unwrap();
  std::fs::create_dir_all(&cache).unwrap();
  let file =
    OpenOptions::new().write(true).create(true).append(true).open(cache.join("main.log")).unwrap();

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
    .chain(std::io::stdout())
    .chain(file)
    .apply()
    .unwrap();

  let _s = if std::env::var("BE_PROFILE").is_ok() {
    puffin::set_scopes_on(true);
    Some(puffin_http::Server::new("127.0.0.1:8585").unwrap())
  } else {
    None
  };

  be_gui::run();
}
