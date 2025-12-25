use std::fs::OpenOptions;

use fern::Dispatch;
use log::LevelFilter;

fn main() {
  let cache = std::env::home_dir().unwrap().join(".cache");
  std::fs::create_dir_all(cache.join("be")).unwrap();
  let file = OpenOptions::new()
    .write(true)
    .create(true)
    .append(true)
    .open(cache.join("be").join("main.log"))
    .unwrap();

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
    .chain(std::io::stdout())
    .chain(file)
    .apply()
    .unwrap();

  be_gui::run();
}
